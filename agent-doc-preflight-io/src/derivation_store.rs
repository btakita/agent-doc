//! First-principles derivations in the decision graph
//! (`#firstprinciplesstore`).
//!
//! [`derive`] authors one inference step (`agent-doc decision derive`): it
//! resolves each premise (`principle:<id>` against the principles in scope,
//! `fact:<id>[=<statement>]`, `decision:<id>` against the document's decision
//! graph), resolves the decision the step concludes, and stores the step in
//! the `state.db` decision-graph tables
//! ([`agent_doc_sqlite::answer_ledger_derivations`]) as
//! `agent_derived_unreviewed`.
//!
//! Only the operator's words change a step's status: a `confirm [#drv-…]` /
//! `reject [#drv-…]: <why>` row ([`agent_doc_answer_ledger::derivation_review`]).
//! [`load`] overlays the reviews the document shows onto the stored steps, and
//! every preflight ledger pass records them ([`apply_reviews`]), so a review
//! survives after its row is pruned. Rejecting a step never rejects its
//! premises.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use agent_doc_answer_ledger::derivation_review::{
    DERIVATION_ID_PREFIX, DerivationReviewObs, Verdict, effective_reviews, is_derivation_id,
};
use agent_doc_answer_ledger::graph::{DecisionGraph, NodeKind};
use agent_doc_principles::Principle;
use agent_doc_principles::derivation::{
    Conclusion, Contradiction, Derivation, DerivationStatus, Graph, NodeKind as PremiseKind,
    NodeRef,
};
use agent_doc_sqlite::answer_ledger_derivations::{
    self as store, DerivationPremiseRow, DerivationRow, StoreOutcome,
};
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;

/// One `--premise` as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PremiseSpec {
    Principle {
        id: String,
    },
    Fact {
        id: String,
        statement: Option<String>,
    },
    Decision {
        id: String,
    },
}

/// Parse `principle:<id>`, `fact:<id>`, `fact:<id>=<statement>` or
/// `decision:<id>` (a leading `#` or `[#…]` around the id is accepted).
pub fn parse_premise(spec: &str) -> Result<PremiseSpec> {
    let spec = spec.trim();
    let (kind, rest) = spec.split_once(':').with_context(|| {
        format!("premise `{spec}` needs a kind: principle:<id>, fact:<id>[=<statement>] or decision:<id>")
    })?;
    let bare = |id: &str| {
        id.trim()
            .trim_start_matches("[#")
            .trim_start_matches('#')
            .trim_end_matches(']')
            .to_string()
    };
    let premise = match kind.trim().to_ascii_lowercase().as_str() {
        "principle" => PremiseSpec::Principle { id: bare(rest) },
        "fact" => match rest.split_once('=') {
            Some((id, statement)) => PremiseSpec::Fact {
                id: bare(id),
                statement: Some(statement.trim().to_string()).filter(|s| !s.is_empty()),
            },
            None => PremiseSpec::Fact {
                id: bare(rest),
                statement: None,
            },
        },
        "decision" => PremiseSpec::Decision { id: bare(rest) },
        other => {
            bail!("premise kind `{other}` is not one of principle, fact, decision (in `{spec}`)")
        }
    };
    let id = match &premise {
        PremiseSpec::Principle { id }
        | PremiseSpec::Fact { id, .. }
        | PremiseSpec::Decision { id } => id,
    };
    ensure!(!id.is_empty(), "premise `{spec}` has no id");
    if !matches!(premise, PremiseSpec::Decision { .. }) {
        ensure!(
            agent_doc_principles::is_valid_id(id),
            "premise `{spec}`: `{id}` is not a valid id"
        );
    }
    Ok(premise)
}

/// What `agent-doc decision derive` asks for.
#[derive(Debug, Clone, Default)]
pub struct DeriveRequest {
    pub premises: Vec<String>,
    pub inference: String,
    /// Decision id, revision id, question copy id, or an item one decision
    /// applies to.
    pub decision: String,
    pub answer: String,
    /// Explicit `drv-…` id; default: derived from the step's content.
    pub id: Option<String>,
}

/// One premise of a stored step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StoredPremise {
    /// `principle`, `fact` or `decision`.
    pub kind: String,
    /// Principle or fact id, or the decision id.
    pub id: String,
    /// Graph node id (`principle:<id>`, `fact:<id>`, or the decision id).
    pub node: String,
    /// Statement, or the decision's question.
    pub label: String,
}

/// One stored step with the status the document and the store give it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StoredDerivation {
    pub id: String,
    pub status: DerivationStatus,
    pub inference: String,
    pub premises: Vec<StoredPremise>,
    pub decision: String,
    pub answer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_seen_at: Option<u64>,
    /// The operator's reason for the review, when given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_note: Option<String>,
    /// Where the deciding operator row is, when the document still shows it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_location: Option<String>,
}

impl StoredDerivation {
    /// The pure model form, for why / impact / contradiction queries.
    pub fn to_model(&self) -> Derivation {
        Derivation {
            id: self.id.clone(),
            premises: self
                .premises
                .iter()
                .map(|premise| {
                    let kind = match premise.kind.as_str() {
                        "principle" => PremiseKind::Principle,
                        "fact" => PremiseKind::Fact,
                        _ => PremiseKind::Decision,
                    };
                    NodeRef::new(kind, premise.id.clone())
                })
                .collect(),
            inference: self.inference.clone(),
            conclusion: Conclusion {
                decision: self.decision.clone(),
                answer: self.answer.clone(),
            },
            status: self.status,
        }
    }
}

/// What [`derive`] did.
#[derive(Debug, Clone, Serialize)]
pub struct DeriveOutcome {
    pub derivation: StoredDerivation,
    /// False when the same step was already on record (its status kept).
    pub created: bool,
    /// Live derivations of the same decision that reach another answer.
    pub contradictions: Vec<Contradiction>,
    /// The operator forms that confirm or reject this step.
    pub review_forms: Vec<String>,
}

pub fn status_str(status: DerivationStatus) -> &'static str {
    match status {
        DerivationStatus::OperatorConfirmed => store::STATUS_CONFIRMED,
        DerivationStatus::AgentDerivedUnreviewed => store::STATUS_UNREVIEWED,
        DerivationStatus::Rejected => store::STATUS_REJECTED,
    }
}

fn status_of(value: &str) -> DerivationStatus {
    match value {
        store::STATUS_CONFIRMED => DerivationStatus::OperatorConfirmed,
        store::STATUS_REJECTED => DerivationStatus::Rejected,
        _ => DerivationStatus::AgentDerivedUnreviewed,
    }
}

fn verdict_status(verdict: Verdict) -> DerivationStatus {
    match verdict {
        Verdict::Confirm => DerivationStatus::OperatorConfirmed,
        Verdict::Reject => DerivationStatus::Rejected,
    }
}

/// The operator forms for one step.
pub fn review_forms(id: &str) -> Vec<String> {
    vec![format!("confirm [#{id}]"), format!("reject [#{id}]: <why>")]
}

fn store_target(file: &Path) -> Result<(PathBuf, String)> {
    let root = agent_doc_project_root_io::project_root_containing(file).with_context(|| {
        format!(
            "{} is not under an agent-doc project (no .agent-doc/ directory); derivations live in its state.db",
            file.display()
        )
    })?;
    Ok((root, agent_doc_hash::document_id_for_path(file)))
}

fn stored_of(row: DerivationRow) -> StoredDerivation {
    StoredDerivation {
        status: status_of(&row.status),
        premises: row
            .premises
            .into_iter()
            .map(|premise| StoredPremise {
                id: premise
                    .node_id
                    .strip_prefix(&format!("{}:", premise.kind))
                    .unwrap_or(&premise.node_id)
                    .to_string(),
                kind: premise.kind,
                node: premise.node_id,
                label: premise.label,
            })
            .collect(),
        id: row.id,
        inference: row.inference,
        decision: row.decision_id,
        answer: row.answer,
        first_seen_at: row.first_seen_at,
        review_note: row.status_note,
        review_location: None,
    }
}

fn overlay(stored: &mut [StoredDerivation], reviews: &[DerivationReviewObs]) {
    let effective = effective_reviews(reviews);
    for derivation in stored {
        if let Some(review) = effective.get(&derivation.id) {
            derivation.status = verdict_status(review.verdict);
            derivation.review_note = review.note.clone();
            derivation.review_location = Some(review.location.clone());
        }
    }
}

fn load_rows(file: &Path) -> Result<Vec<DerivationRow>> {
    let (root, document_id) = store_target(file)?;
    match store::load_derivations_for_project(&root, &document_id) {
        None => Ok(Vec::new()),
        Some(rows) => rows,
    }
}

/// Every stored step for `file`, with the reviews `content` shows applied.
/// Read-only.
pub fn load(file: &Path, content: &str) -> Result<Vec<StoredDerivation>> {
    let rows = load_rows(file)?;
    let mut stored: Vec<StoredDerivation> = rows.into_iter().map(stored_of).collect();
    if !stored.is_empty() {
        let (observations, _) = crate::answer_ledger::build_ledger(file, content);
        overlay(&mut stored, &observations.derivation_reviews);
    }
    Ok(stored)
}

/// The decision `id` names in `graph`: a decision, revision or question copy
/// id, or an item exactly one decision applies to.
fn resolve_decision(graph: &DecisionGraph, id: &str) -> Result<String> {
    if let Some(decision) = graph.resolve_decision(id) {
        return Ok(decision);
    }
    let bare = id
        .trim()
        .trim_start_matches("[#")
        .trim_start_matches('#')
        .trim_end_matches(']');
    match graph.decisions_applying_to(bare).as_slice() {
        [one] => Ok(one.clone()),
        [] => bail!(
            "{id} is not a decision, a revision, a question copy, or an item a decision applies to in this document"
        ),
        many => bail!(
            "{id} is an item several decisions apply to; name one: {}",
            many.join(", ")
        ),
    }
}

fn decision_label(graph: &DecisionGraph, decision: &str) -> String {
    graph
        .nodes
        .get(decision)
        .map(|node| node.label.clone())
        .unwrap_or_default()
}

/// The content-derived id of one step.
pub fn derivation_id(decision: &str, answer: &str, inference: &str, premises: &[String]) -> String {
    let mut premises = premises.to_vec();
    premises.sort();
    let canonical = format!(
        "{decision}\n{}\n{}\n{}",
        agent_doc_principles::derivation::normalize_answer(answer),
        inference.split_whitespace().collect::<Vec<_>>().join(" "),
        premises.join("\n")
    );
    format!(
        "{DERIVATION_ID_PREFIX}{}",
        &agent_doc_hash::content_hash(&canonical)[..10]
    )
}

/// Author one step and store it (`agent-doc decision derive`). `principles`
/// are the principles in scope for `file` (global, project, document).
pub fn derive(
    file: &Path,
    content: &str,
    principles: &[Principle],
    request: &DeriveRequest,
) -> Result<DeriveOutcome> {
    let inference = request.inference.trim();
    let answer = request.answer.trim();
    ensure!(
        !inference.is_empty(),
        "--inference states no inference step"
    );
    ensure!(!answer.is_empty(), "--answer is empty");
    ensure!(
        !request.premises.is_empty(),
        "a derivation needs at least one --premise"
    );
    let (root, document_id) = store_target(file)?;
    let (_, graph) = crate::answer_ledger::build_graph(file, content);
    let decision = resolve_decision(&graph, &request.decision)?;
    ensure!(
        graph
            .nodes
            .get(&decision)
            .is_some_and(|node| node.kind == NodeKind::Decision),
        "{decision} is not a decision node"
    );
    let stored_facts: BTreeMap<String, String> = graph
        .nodes
        .values()
        .filter(|node| node.kind == NodeKind::Fact)
        .map(|node| (node.id.clone(), node.label.clone()))
        .collect();

    let mut premises: Vec<DerivationPremiseRow> = Vec::new();
    for spec in &request.premises {
        let premise = match parse_premise(spec)? {
            PremiseSpec::Principle { id } => {
                let principle = principles
                    .iter()
                    .find(|principle| principle.id.eq_ignore_ascii_case(&id))
                    .with_context(|| {
                        let known: Vec<&str> = principles
                            .iter()
                            .map(|principle| principle.id.as_str())
                            .collect();
                        format!(
                            "no principle [#{id}] is in scope for {} (in scope: {})",
                            file.display(),
                            if known.is_empty() {
                                "none".to_string()
                            } else {
                                known.join(", ")
                            }
                        )
                    })?;
                DerivationPremiseRow {
                    node_id: format!("principle:{}", principle.id),
                    kind: "principle".into(),
                    label: principle.statement.clone(),
                }
            }
            PremiseSpec::Fact { id, statement } => {
                let node_id = format!("fact:{id}");
                let label = match statement {
                    Some(statement) => statement,
                    None => stored_facts.get(&node_id).cloned().with_context(|| {
                        format!("fact {id} has no statement on record; write --premise 'fact:{id}=<statement>'")
                    })?,
                };
                DerivationPremiseRow {
                    node_id,
                    kind: "fact".into(),
                    label,
                }
            }
            PremiseSpec::Decision { id } => {
                let premise = resolve_decision(&graph, &id)?;
                ensure!(
                    premise != decision,
                    "decision:{id} is the decision this step concludes; a step cannot be its own premise"
                );
                DerivationPremiseRow {
                    label: decision_label(&graph, &premise),
                    node_id: premise,
                    kind: "decision".into(),
                }
            }
        };
        if !premises.iter().any(|p| p.node_id == premise.node_id) {
            premises.push(premise);
        }
    }

    let premise_ids: Vec<String> = premises.iter().map(|p| p.node_id.clone()).collect();
    let id = match &request.id {
        Some(id) => {
            let id = id
                .trim()
                .trim_start_matches("[#")
                .trim_start_matches('#')
                .trim_end_matches(']')
                .to_ascii_lowercase();
            ensure!(
                is_derivation_id(&id),
                "--id {id} must start with `{DERIVATION_ID_PREFIX}` and use letters, digits, `-` or `_`"
            );
            id
        }
        None => derivation_id(&decision, answer, inference, &premise_ids),
    };
    let row = DerivationRow {
        id: id.clone(),
        inference: inference.to_string(),
        status: store::STATUS_UNREVIEWED.to_string(),
        premises,
        decision_label: decision_label(&graph, &decision),
        decision_id: decision.clone(),
        answer: answer.to_string(),
        first_seen_at: None,
        status_at: None,
        status_note: None,
    };
    // The pure model's own checks (no premises, empty inference, a step that
    // is its own premise) before anything is written.
    Graph::new([stored_of(row.clone()).to_model()]).map_err(|err| anyhow::anyhow!("{err}"))?;
    let now = agent_doc_log_time::current_epoch_secs();
    let created = matches!(
        store::store_derivation_for_project(&root, &document_id, &row, now)?,
        StoreOutcome::Created
    );
    let all = load(file, content)?;
    let derivation = all
        .iter()
        .find(|stored| stored.id == id)
        .cloned()
        .with_context(|| format!("derivation {id} was stored but cannot be read back"))?;
    let contradictions = Graph::new(all.iter().map(StoredDerivation::to_model))
        .map(|graph| {
            graph
                .contradictions()
                .into_iter()
                .filter(|contradiction| contradiction.decision == decision)
                .collect()
        })
        .unwrap_or_default();
    Ok(DeriveOutcome {
        review_forms: review_forms(&id),
        derivation,
        created,
        contradictions,
    })
}

/// Record the operator reviews `observations` show (one ledger pass). A
/// review naming a step this document never stored is reported and skipped.
/// Never fails the pass.
pub fn apply_reviews(
    file: &Path,
    observations: &agent_doc_answer_ledger::extract::Observations,
    now: u64,
) {
    if observations.derivation_reviews.is_empty() {
        return;
    }
    let Ok((root, document_id)) = store_target(file) else {
        return;
    };
    let rows = match load_rows(file) {
        Ok(rows) => rows,
        Err(err) => {
            eprintln!("[preflight] derivations: store unreadable ({err:#}); reviews not recorded");
            return;
        }
    };
    for (id, review) in effective_reviews(&observations.derivation_reviews) {
        if !rows.iter().any(|row| row.id == id) {
            eprintln!(
                "[preflight] derivations: {} [#{id}] ({}) names no stored derivation of this document",
                review.verdict.as_str(),
                review.location
            );
            continue;
        }
        let status = status_str(verdict_status(review.verdict));
        match store::set_derivation_status_for_project(
            &root,
            &document_id,
            &id,
            status,
            review.note.as_deref(),
            now,
        ) {
            Ok(true) => {
                eprintln!(
                    "[preflight] derivations: [#{id}] is now {status} ({})",
                    review.location
                );
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "derivation_reviewed file={} derivation={id} status={status}",
                        file.display()
                    ),
                );
            }
            Ok(false) => {}
            Err(err) => eprintln!("[preflight] derivations: [#{id}] review not recorded ({err:#})"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premise_specs() {
        assert_eq!(
            parse_premise("principle:[#simple]").unwrap(),
            PremiseSpec::Principle {
                id: "simple".into()
            }
        );
        assert_eq!(
            parse_premise("fact:jws=Credentials are JWS.").unwrap(),
            PremiseSpec::Fact {
                id: "jws".into(),
                statement: Some("Credentials are JWS.".into())
            }
        );
        assert_eq!(
            parse_premise("fact:jws").unwrap(),
            PremiseSpec::Fact {
                id: "jws".into(),
                statement: None
            }
        );
        assert_eq!(
            parse_premise("decision:dec-abc").unwrap(),
            PremiseSpec::Decision {
                id: "dec-abc".into()
            }
        );
        assert!(parse_premise("simple").is_err());
        assert!(parse_premise("standing:DEC-1").is_err());
        assert!(parse_premise("fact:").is_err());
    }

    #[test]
    fn the_content_id_ignores_premise_order_and_spacing() {
        let a = derivation_id(
            "dec-a",
            "No cap.",
            "JWS  expires itself.",
            &["principle:simple".into(), "fact:jws".into()],
        );
        let b = derivation_id(
            "dec-a",
            "no cap.",
            "JWS expires itself.",
            &["fact:jws".into(), "principle:simple".into()],
        );
        assert_eq!(a, b);
        assert!(a.starts_with("drv-") && a.len() == 14, "{a}");
        assert_ne!(
            a,
            derivation_id(
                "dec-a",
                "Cap it.",
                "JWS expires itself.",
                &["fact:jws".into()]
            )
        );
    }

    #[test]
    fn the_document_review_overrides_the_stored_status() {
        let mut stored = vec![StoredDerivation {
            id: "drv-1".into(),
            status: DerivationStatus::AgentDerivedUnreviewed,
            inference: "i".into(),
            premises: Vec::new(),
            decision: "dec-a".into(),
            answer: "a".into(),
            first_seen_at: None,
            review_note: None,
            review_location: None,
        }];
        overlay(
            &mut stored,
            &[DerivationReviewObs {
                derivation: "drv-1".into(),
                verdict: Verdict::Reject,
                note: Some("wrong premise".into()),
                location: "queue row".into(),
                line: 3,
                consumed: false,
                archived: false,
            }],
        );
        assert_eq!(stored[0].status, DerivationStatus::Rejected);
        assert_eq!(stored[0].review_note.as_deref(), Some("wrong premise"));
        assert_eq!(stored[0].review_location.as_deref(), Some("queue row"));
    }
}
