//! The first-principles chain behind `decision why` (`#firstprincipleswhy`).
//!
//! [`DecisionGraph::why`] answers "why does this decision hold?" from the
//! ledger: the operator's verbatim revisions, evidence and `depends_on`
//! premises. `#firstprinciples` adds derivations: premises (principles,
//! standing decisions, decisions, facts) linked through a stated inference
//! to a decision's answer, walked by [`agent_doc_principles::derivation::Graph::why`].
//! This module joins the two into one answer, [`ReasonedWhy`].
//!
//! Derivations come from two places:
//!
//! - **Operator-stated** ([`operator_derivations`]): a decision's current
//!   answer that cites an in-scope principle as `[#id]`, or a decision the
//!   operator linked with `depends on [#id]` to a principle or another
//!   decision. The operator's own words are the inference, so the step is
//!   `operator_confirmed`. Only revisions that still hold count: a
//!   superseded answer no longer supports the decision; an amended one does.
//! - **Stored** derivations, passed in by the caller (agent derivations and
//!   their review status). Their decision references are resolved onto the
//!   ledger's decision ids, and a stored derivation wins over an
//!   operator-stated one with the same id.
//!
//! Invalid derivations (no premises, no inference, the conclusion as its own
//! premise, a duplicate id) are skipped and reported, never fatal: `why` is a
//! lookup and must still answer from the ledger.

use std::collections::{BTreeMap, BTreeSet};

use agent_doc_principles::Principle;
use agent_doc_principles::derivation::{
    Conclusion, Derivation, DerivationStatus, Graph, NodeKind as PremiseKind, NodeRef, WhyStep,
};
use serde::Serialize;

use super::{DecisionGraph, NodeKind, Why};
use crate::EdgeKind;

/// Id prefix of an operator-stated derivation: `op:<revision id>`.
pub const OPERATOR_DERIVATION_PREFIX: &str = "op:";

/// `decision why` for one decision: the ledger's answer plus the
/// first-principles derivation chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReasonedWhy {
    #[serde(flatten)]
    pub why: Why,
    /// Derivation steps concluding this decision, recursing through premises
    /// that are themselves derived decisions down to principles and facts.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub derived_from: Vec<WhyStep>,
    /// At least one live (not rejected) derivation supports the decision.
    /// Absent when no derivation concludes it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub derivation_supported: Option<bool>,
    /// Every in-scope principle the chain or `depends_on` reaches, first
    /// reached first, with its statement and the file it was read from.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub principles: Vec<Principle>,
    /// Principle premises with no principle of that id in scope (removed or
    /// renamed since the derivation was recorded).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing_principles: Vec<String>,
}

/// Principles in scope, keyed by lowercase id.
struct Principles<'a>(BTreeMap<String, &'a Principle>);

impl<'a> Principles<'a> {
    fn new(principles: &'a [Principle]) -> Self {
        Self(
            principles
                .iter()
                .map(|principle| (principle.id.to_ascii_lowercase(), principle))
                .collect(),
        )
    }

    fn get(&self, id: &str) -> Option<&'a Principle> {
        self.0.get(&id.to_ascii_lowercase()).copied()
    }
}

/// In-scope principle ids cited as `[#id]` in `text`, in order, each once,
/// spelled as the principle declares it.
pub fn principle_citations(text: &str, principles: &[Principle]) -> Vec<String> {
    let principles = Principles::new(principles);
    cited(text, &principles)
}

fn cited(text: &str, principles: &Principles<'_>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("[#") {
        let after = &rest[start + 2..];
        let Some(end) = after.find(']') else { break };
        if let Some(principle) = principles.get(&after[..end])
            && !out.contains(&principle.id)
        {
            out.push(principle.id.clone());
        }
        rest = &after[end..];
    }
    out
}

fn push_unique(premises: &mut Vec<NodeRef>, premise: NodeRef) {
    if !premises.contains(&premise) {
        premises.push(premise);
    }
}

/// Derivations the operator stated in the decision graph: for each decision
/// with an answer, the `[#principle]` citations in the revisions that still
/// hold (the latest one back through any amends and restatements to the one
/// that superseded or began the answer) plus its `depends_on` links (from the
/// decision or any copy) to principles and other decisions. Those revisions,
/// verbatim, are the inference; the latest is the conclusion; the status is
/// `operator_confirmed`. The id is `op:<latest revision id>`.
pub fn operator_derivations(graph: &DecisionGraph, principles: &[Principle]) -> Vec<Derivation> {
    let principles = Principles::new(principles);
    let mut out = Vec::new();
    for node in graph.nodes.values() {
        if node.kind != NodeKind::Decision {
            continue;
        }
        let revisions = graph.revisions_of(&node.id);
        let Some(current) = revisions.last().copied() else {
            continue;
        };
        // The revisions that still hold: back to the latest one that
        // superseded (or began) the answer; amends and restates keep the
        // revisions before them.
        let start = revisions
            .iter()
            .rposition(|revision| {
                graph
                    .relation_of(&revision.id)
                    .is_none_or(|edge| edge.kind == EdgeKind::Supersedes)
            })
            .unwrap_or(0);
        let holding = &revisions[start..];
        let members: Vec<String> = std::iter::once(node.id.clone())
            .chain(
                graph
                    .incoming(&node.id, EdgeKind::CopyOf)
                    .map(|edge| edge.from.clone()),
            )
            .collect();
        let mut premises = Vec::new();
        for revision in holding {
            for id in cited(&revision.label, &principles) {
                push_unique(&mut premises, NodeRef::new(PremiseKind::Principle, id));
            }
        }
        for member in &members {
            for edge in graph.outgoing(member, EdgeKind::DependsOn) {
                if let Some(principle) = principles.get(&edge.to) {
                    push_unique(
                        &mut premises,
                        NodeRef::new(PremiseKind::Principle, principle.id.clone()),
                    );
                } else if edge.to != node.id
                    && graph
                        .nodes
                        .get(&edge.to)
                        .is_some_and(|target| target.kind == NodeKind::Decision)
                {
                    push_unique(
                        &mut premises,
                        NodeRef::new(PremiseKind::Decision, edge.to.clone()),
                    );
                }
            }
        }
        if premises.is_empty() {
            continue;
        }
        out.push(Derivation {
            id: format!("{OPERATOR_DERIVATION_PREFIX}{}", current.id),
            premises,
            inference: holding
                .iter()
                .map(|revision| revision.label.as_str())
                .collect::<Vec<_>>()
                .join("; "),
            conclusion: Conclusion {
                decision: node.id.clone(),
                answer: current.label.clone(),
            },
            status: DerivationStatus::OperatorConfirmed,
        });
    }
    out
}

/// The derivation graph behind `decision why`: `stored` derivations (their
/// decision references resolved onto ledger decision ids) and the
/// [`operator_derivations`]. Returns the graph and one message per skipped
/// derivation.
pub fn derivation_graph(
    graph: &DecisionGraph,
    principles: &[Principle],
    stored: impl IntoIterator<Item = Derivation>,
) -> (Graph, Vec<String>) {
    let resolve = |id: &str| graph.resolve_decision(id).unwrap_or_else(|| id.to_string());
    let mut skipped = Vec::new();
    let mut ids = BTreeSet::new();
    let mut kept = Vec::new();
    let stored = stored.into_iter().map(|mut derivation| {
        derivation.conclusion.decision = resolve(&derivation.conclusion.decision);
        for premise in &mut derivation.premises {
            if premise.kind.is_decision() {
                premise.id = resolve(&premise.id);
            }
        }
        derivation
    });
    for derivation in stored.chain(operator_derivations(graph, principles)) {
        let problem = if ids.contains(&derivation.id) {
            Some("is declared twice")
        } else if derivation.premises.is_empty() {
            Some("has no premises")
        } else if derivation.inference.trim().is_empty() {
            Some("states no inference step")
        } else if derivation.premises.iter().any(|premise| {
            premise.kind.is_decision() && premise.id == derivation.conclusion.decision
        }) {
            Some("lists its own conclusion as a premise")
        } else {
            None
        };
        if let Some(problem) = problem {
            // An operator-stated id repeating a stored one is the stored
            // record winning, not a problem.
            if !(problem == "is declared twice"
                && derivation.id.starts_with(OPERATOR_DERIVATION_PREFIX))
            {
                skipped.push(format!("derivation {} {problem}; skipped", derivation.id));
            }
            continue;
        }
        ids.insert(derivation.id.clone());
        kept.push(derivation);
    }
    // Every refusal `Graph::new` knows was filtered above.
    let graph = Graph::new(kept).unwrap_or_default();
    (graph, skipped)
}

fn collect_principles(steps: &[WhyStep], out: &mut Vec<String>) {
    for step in steps {
        for premise in &step.premises {
            if premise.node.kind == PremiseKind::Principle && !out.contains(&premise.node.id) {
                out.push(premise.node.id.clone());
            }
            if let Some(because) = &premise.because {
                collect_principles(&because.steps, out);
            }
        }
    }
}

/// Why `decision` holds, from the ledger and the derivation chain. A
/// decision only a derivation concludes (no ledger node) still answers, with
/// the live derivations' answer as its current answer. `None` when neither
/// knows the decision.
pub fn reasoned_why(
    graph: &DecisionGraph,
    derivations: &Graph,
    principles: &[Principle],
    decision: &str,
) -> Option<ReasonedWhy> {
    let chain = derivations.why(decision);
    let mut why = match graph.why(decision) {
        Some(why) => why,
        None if !chain.steps.is_empty() => Why {
            decision_id: decision.to_string(),
            question: String::new(),
            current_answer: chain
                .steps
                .iter()
                .find(|step| step.status.is_live())
                .map(|step| step.answer.clone()),
            revisions: Vec::new(),
            copies: Vec::new(),
            depends_on: Vec::new(),
            conflicts_with: Vec::new(),
            applies_to: Vec::new(),
            evidence: Vec::new(),
            merged: Vec::new(),
            supersedes: Vec::new(),
            superseded_by: Vec::new(),
        },
        None => return None,
    };
    let in_scope = Principles::new(principles);
    let mut reached: Vec<String> = Vec::new();
    // A `depends on [#principle]` link is a principle premise, not an item.
    for premise in &mut why.depends_on {
        if let Some(principle) = in_scope.get(&premise.id) {
            premise.kind = NodeKind::Principle;
            premise.label = principle.statement.clone();
            if !reached.contains(&principle.id) {
                reached.push(principle.id.clone());
            }
        }
    }
    collect_principles(&chain.steps, &mut reached);
    let mut found = Vec::new();
    let mut missing = Vec::new();
    for id in reached {
        match in_scope.get(&id) {
            Some(principle) => {
                if !found.iter().any(|p: &Principle| p.id == principle.id) {
                    found.push(principle.clone());
                }
            }
            None => missing.push(id),
        }
    }
    let derivation_supported = (!chain.steps.is_empty()).then(|| chain.supported());
    Some(ReasonedWhy {
        why,
        derived_from: chain.steps,
        derivation_supported,
        principles: found,
        missing_principles: missing,
    })
}

fn status_str(status: DerivationStatus) -> &'static str {
    match status {
        DerivationStatus::OperatorConfirmed => "operator_confirmed",
        DerivationStatus::AgentDerivedUnreviewed => "agent_derived_unreviewed",
        DerivationStatus::Rejected => "rejected",
    }
}

fn quote(text: &str) -> String {
    format!("“{text}”")
}

fn render_steps(
    steps: &[WhyStep],
    principles: &BTreeMap<&str, &Principle>,
    depth: usize,
    lines: &mut Vec<String>,
) {
    let indent = "  ".repeat(depth);
    for step in steps {
        let conclusion = if step.answer == step.inference {
            quote(&step.inference)
        } else {
            format!("{} ⇒ {}", quote(&step.inference), quote(&step.answer))
        };
        lines.push(format!(
            "{indent}{} ({}): {conclusion}",
            step.derivation,
            status_str(step.status)
        ));
        for premise in &step.premises {
            let kind = premise.node.kind.as_str();
            let id = &premise.node.id;
            let mut line = format!("{indent}  {kind} {id}");
            if premise.node.kind == PremiseKind::Principle {
                match principles.get(id.as_str()) {
                    Some(principle) => line.push_str(&format!(" {}", quote(&principle.statement))),
                    None => line.push_str(" (not in scope)"),
                }
            }
            if premise.cycle {
                line.push_str(" (cycle: already on this chain)");
            }
            lines.push(line);
            if let Some(because) = &premise.because {
                render_steps(&because.steps, principles, depth + 2, lines);
            }
        }
    }
}

/// Text rendering of one [`ReasonedWhy`] for `agent-doc decision why`.
pub fn render_reasoned_why(reasoned: &ReasonedWhy) -> Vec<String> {
    let why = &reasoned.why;
    let mut lines = Vec::new();
    lines.push(
        format!("{} {}", why.decision_id, why.question)
            .trim_end()
            .to_string(),
    );
    if let Some(answer) = &why.current_answer {
        lines.push(format!("  holds: {}", quote(answer)));
    }
    for revision in &why.revisions {
        lines.push(format!(
            "  {} ({}) {} — {}{}",
            revision.relation,
            revision.relation_source.as_str(),
            quote(&revision.answer),
            revision.location.as_deref().unwrap_or("unknown location"),
            if revision.evidence.is_empty() {
                String::new()
            } else {
                format!("; evidence: {}", revision.evidence.join(", "))
            },
        ));
    }
    for premise in &why.depends_on {
        let kind = if premise.kind == NodeKind::Principle {
            "principle "
        } else {
            ""
        };
        lines.push(format!(
            "  depends on {kind}{} {}{}",
            premise.id,
            quote(&premise.label),
            premise
                .answer
                .as_deref()
                .map(|answer| format!(": {}", quote(answer)))
                .unwrap_or_default()
        ));
    }
    if !why.conflicts_with.is_empty() {
        lines.push(format!(
            "  conflicts with: {}",
            why.conflicts_with.join(", ")
        ));
    }
    for old in &why.supersedes {
        lines.push(format!(
            "  supersedes {}{}",
            old.id,
            old.answer
                .as_deref()
                .map(|answer| format!(": {}", quote(answer)))
                .unwrap_or_default()
        ));
    }
    if !why.superseded_by.is_empty() {
        lines.push(format!("  superseded by: {}", why.superseded_by.join(", ")));
    }
    if !why.copies.is_empty() {
        lines.push(format!("  copies: {}", why.copies.join(", ")));
    }
    if !why.applies_to.is_empty() {
        lines.push(format!("  applies to: {}", why.applies_to.join(", ")));
    }
    if !reasoned.derived_from.is_empty() {
        let principles: BTreeMap<&str, &Principle> = reasoned
            .principles
            .iter()
            .map(|principle| (principle.id.as_str(), principle))
            .collect();
        lines.push(if reasoned.derivation_supported == Some(false) {
            "  derived from (every step rejected; nothing live supports it):".to_string()
        } else {
            "  derived from:".to_string()
        });
        render_steps(&reasoned.derived_from, &principles, 2, &mut lines);
    }
    for principle in &reasoned.principles {
        lines.push(format!(
            "  principle {} ({}, {}): {}",
            principle.id,
            principle.scope.as_str(),
            principle.source,
            quote(&principle.statement)
        ));
    }
    if !reasoned.missing_principles.is_empty() {
        lines.push(format!(
            "  principles no longer in scope: {}",
            reasoned.missing_principles.join(", ")
        ));
    }
    lines
}

#[cfg(test)]
mod tests;
