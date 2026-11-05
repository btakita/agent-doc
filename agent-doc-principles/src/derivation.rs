//! Derivations: how a decision follows from principles and facts.
//!
//! A [`Derivation`] links premises (principles, standing decisions, earlier
//! decisions, and facts such as "credentials are JWS") through one stated
//! inference step to a conclusion (an answer to a decision). Its status says
//! whether the operator confirmed it, whether it is an unreviewed agent
//! derivation, or whether the operator rejected that inference step. Rejecting
//! a step never rejects its premises.
//!
//! [`Graph`] answers three questions over a set of derivations:
//!
//! - [`Graph::why`]: "why was this decided?" walks decision ← inference ←
//!   premises, recursing through premises that are themselves derived
//!   decisions. Cycle-safe.
//! - [`Graph::impact`]: a premise changed; which conclusions depended on it,
//!   directly or through other derived decisions? An unreviewed agent
//!   derivation is re-derived; an operator-confirmed one is flagged for the
//!   operator, never silently rewritten.
//! - [`Graph::contradictions`]: two live derivations of the same decision that
//!   reach different answers. Same premises means the reasoning is
//!   inconsistent; different premises means the premises conflict.
//!
//! The model is storage-agnostic: ids are strings, so the answer ledger's
//! `dec-…` ids, `decisions.md` `DEC-…` ids, and principle ids all fit.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// What a premise is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Principle,
    StandingDecision,
    Decision,
    Fact,
}

impl NodeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Principle => "principle",
            Self::StandingDecision => "standing_decision",
            Self::Decision => "decision",
            Self::Fact => "fact",
        }
    }

    /// Decisions (standing or not) can be conclusions of other derivations.
    pub const fn is_decision(self) -> bool {
        matches!(self, Self::StandingDecision | Self::Decision)
    }
}

/// A premise reference.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NodeRef {
    pub kind: NodeKind,
    pub id: String,
}

impl NodeRef {
    pub fn new(kind: NodeKind, id: impl Into<String>) -> Self {
        Self {
            kind,
            id: id.into(),
        }
    }
}

/// Review status of one inference step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DerivationStatus {
    /// The operator confirmed this inference.
    OperatorConfirmed,
    /// The agent derived it; the operator has not reviewed it.
    AgentDerivedUnreviewed,
    /// The operator rejected this inference step (its premises stand).
    Rejected,
}

impl DerivationStatus {
    pub const fn is_live(self) -> bool {
        !matches!(self, Self::Rejected)
    }
}

/// The conclusion of a derivation: an answer to a decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Conclusion {
    pub decision: String,
    pub answer: String,
}

/// One inference step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Derivation {
    pub id: String,
    pub premises: Vec<NodeRef>,
    pub inference: String,
    pub conclusion: Conclusion,
    pub status: DerivationStatus,
}

/// Why a set of derivations cannot form a graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphError {
    DuplicateId(String),
    NoPremises(String),
    EmptyInference(String),
    /// A derivation lists its own conclusion as a premise.
    SelfPremise(String),
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateId(id) => write!(f, "derivation {id} is declared twice"),
            Self::NoPremises(id) => write!(f, "derivation {id} has no premises"),
            Self::EmptyInference(id) => write!(f, "derivation {id} states no inference step"),
            Self::SelfPremise(id) => {
                write!(f, "derivation {id} lists its own conclusion as a premise")
            }
        }
    }
}

impl std::error::Error for GraphError {}

/// A validated set of derivations.
#[derive(Debug, Clone, Default)]
pub struct Graph {
    derivations: BTreeMap<String, Derivation>,
    /// decision id -> derivation ids concluding it, in id order.
    by_conclusion: BTreeMap<String, Vec<String>>,
}

/// One "because" step in a why-chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WhyStep {
    pub derivation: String,
    pub status: DerivationStatus,
    pub inference: String,
    pub answer: String,
    pub premises: Vec<WhyPremise>,
}

/// A premise in a why-chain, with its own chain when it is a derived decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WhyPremise {
    pub node: NodeRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub because: Option<Why>,
    /// The premise is already on the chain above it (a reasoning cycle).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub cycle: bool,
}

/// The answer to "why was this decided?".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Why {
    pub decision: String,
    pub steps: Vec<WhyStep>,
}

impl Why {
    /// At least one live (not rejected) derivation supports the decision.
    pub fn supported(&self) -> bool {
        self.steps.iter().any(|step| step.status.is_live())
    }
}

/// What to do with a conclusion whose premise changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImpactAction {
    /// An unreviewed agent derivation: derive it again from current premises.
    Rederive,
    /// An operator-confirmed derivation: show the operator; never rewrite it.
    Flag,
}

/// One conclusion affected by a changed premise.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Impact {
    pub derivation: String,
    pub decision: String,
    pub action: ImpactAction,
    /// Node ids from the changed premise to this derivation's premise, inclusive.
    pub via: Vec<String>,
}

/// Which way two derivations of one decision disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContradictionKind {
    /// Identical premises, different answers: an inference step is wrong.
    SamePremises,
    /// Different premises, different answers: the premises conflict.
    DifferentPremises,
}

/// Two or more live derivations of one decision reaching different answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contradiction {
    pub decision: String,
    pub kind: ContradictionKind,
    /// `(derivation id, answer)` in derivation id order.
    pub derivations: Vec<(String, String)>,
}

impl Graph {
    pub fn new(derivations: impl IntoIterator<Item = Derivation>) -> Result<Self, GraphError> {
        let mut graph = Self::default();
        for derivation in derivations {
            if derivation.premises.is_empty() {
                return Err(GraphError::NoPremises(derivation.id));
            }
            if derivation.inference.trim().is_empty() {
                return Err(GraphError::EmptyInference(derivation.id));
            }
            if derivation.premises.iter().any(|premise| {
                premise.kind.is_decision() && premise.id == derivation.conclusion.decision
            }) {
                return Err(GraphError::SelfPremise(derivation.id));
            }
            if graph.derivations.contains_key(&derivation.id) {
                return Err(GraphError::DuplicateId(derivation.id));
            }
            graph
                .by_conclusion
                .entry(derivation.conclusion.decision.clone())
                .or_default()
                .push(derivation.id.clone());
            graph.derivations.insert(derivation.id.clone(), derivation);
        }
        for ids in graph.by_conclusion.values_mut() {
            ids.sort();
        }
        Ok(graph)
    }

    pub fn get(&self, id: &str) -> Option<&Derivation> {
        self.derivations.get(id)
    }

    /// Walk the derivations of `decision`, recursing into derived premises.
    pub fn why(&self, decision: &str) -> Why {
        let mut path = BTreeSet::new();
        self.why_inner(decision, &mut path)
    }

    fn why_inner(&self, decision: &str, path: &mut BTreeSet<String>) -> Why {
        path.insert(decision.to_string());
        let steps = self
            .by_conclusion
            .get(decision)
            .into_iter()
            .flatten()
            .filter_map(|id| self.derivations.get(id))
            .map(|derivation| WhyStep {
                derivation: derivation.id.clone(),
                status: derivation.status,
                inference: derivation.inference.clone(),
                answer: derivation.conclusion.answer.clone(),
                premises: derivation
                    .premises
                    .iter()
                    .map(|premise| {
                        let derived = premise.kind.is_decision()
                            && self.by_conclusion.contains_key(&premise.id);
                        if !derived {
                            return WhyPremise {
                                node: premise.clone(),
                                because: None,
                                cycle: false,
                            };
                        }
                        if path.contains(&premise.id) {
                            return WhyPremise {
                                node: premise.clone(),
                                because: None,
                                cycle: true,
                            };
                        }
                        WhyPremise {
                            node: premise.clone(),
                            because: Some(self.why_inner(&premise.id, path)),
                            cycle: false,
                        }
                    })
                    .collect(),
            })
            .collect();
        path.remove(decision);
        Why {
            decision: decision.to_string(),
            steps,
        }
    }

    /// Every live derivation that depends on `changed`, directly or through a
    /// derived decision, breadth first. Each derivation appears once, at its
    /// shortest path. Rejected derivations do not propagate.
    pub fn impact(&self, changed: &NodeRef) -> Vec<Impact> {
        let mut impacts = Vec::new();
        let mut visited_derivations = BTreeSet::new();
        let mut visited_nodes = BTreeSet::new();
        let mut frontier = vec![(changed.clone(), vec![changed.id.clone()])];
        visited_nodes.insert(changed.clone());
        while !frontier.is_empty() {
            let mut next = Vec::new();
            for (node, via) in frontier {
                for derivation in self.derivations.values() {
                    if !derivation.status.is_live()
                        || visited_derivations.contains(&derivation.id)
                        || !derivation
                            .premises
                            .iter()
                            .any(|premise| premise_matches(premise, &node))
                    {
                        continue;
                    }
                    visited_derivations.insert(derivation.id.clone());
                    impacts.push(Impact {
                        derivation: derivation.id.clone(),
                        decision: derivation.conclusion.decision.clone(),
                        action: match derivation.status {
                            DerivationStatus::OperatorConfirmed => ImpactAction::Flag,
                            _ => ImpactAction::Rederive,
                        },
                        via: via.clone(),
                    });
                    let conclusion =
                        NodeRef::new(NodeKind::Decision, &derivation.conclusion.decision);
                    if visited_nodes.insert(conclusion.clone()) {
                        let mut via = via.clone();
                        via.push(conclusion.id.clone());
                        next.push((conclusion, via));
                    }
                }
            }
            frontier = next;
        }
        impacts
    }

    /// Live derivations of one decision that reach different answers.
    pub fn contradictions(&self) -> Vec<Contradiction> {
        let mut found = Vec::new();
        for (decision, ids) in &self.by_conclusion {
            let live: Vec<&Derivation> = ids
                .iter()
                .filter_map(|id| self.derivations.get(id))
                .filter(|derivation| derivation.status.is_live())
                .collect();
            let answers: BTreeSet<String> = live
                .iter()
                .map(|derivation| normalize_answer(&derivation.conclusion.answer))
                .collect();
            if answers.len() < 2 {
                continue;
            }
            let mut by_premises: BTreeMap<BTreeSet<&NodeRef>, Vec<&Derivation>> = BTreeMap::new();
            for derivation in &live {
                by_premises
                    .entry(derivation.premises.iter().collect())
                    .or_default()
                    .push(derivation);
            }
            let mut same_premise_ids = BTreeSet::new();
            for group in by_premises.values() {
                let group_answers: BTreeSet<String> = group
                    .iter()
                    .map(|derivation| normalize_answer(&derivation.conclusion.answer))
                    .collect();
                if group_answers.len() > 1 {
                    same_premise_ids.extend(group.iter().map(|derivation| derivation.id.clone()));
                    found.push(Contradiction {
                        decision: decision.clone(),
                        kind: ContradictionKind::SamePremises,
                        derivations: pairs(group),
                    });
                }
            }
            if same_premise_ids.len() < live.len() {
                found.push(Contradiction {
                    decision: decision.clone(),
                    kind: ContradictionKind::DifferentPremises,
                    derivations: pairs(&live),
                });
            }
        }
        found
    }
}

fn premise_matches(premise: &NodeRef, node: &NodeRef) -> bool {
    if premise.id != node.id {
        return false;
    }
    premise.kind == node.kind || (premise.kind.is_decision() && node.kind.is_decision())
}

fn pairs(derivations: &[&Derivation]) -> Vec<(String, String)> {
    derivations
        .iter()
        .map(|derivation| (derivation.id.clone(), derivation.conclusion.answer.clone()))
        .collect()
}

/// Case- and whitespace-insensitive answer identity.
pub fn normalize_answer(answer: &str) -> String {
    answer
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn principle(id: &str) -> NodeRef {
        NodeRef::new(NodeKind::Principle, id)
    }
    fn fact(id: &str) -> NodeRef {
        NodeRef::new(NodeKind::Fact, id)
    }
    fn decision(id: &str) -> NodeRef {
        NodeRef::new(NodeKind::Decision, id)
    }

    fn derivation(
        id: &str,
        premises: Vec<NodeRef>,
        decision: &str,
        answer: &str,
        status: DerivationStatus,
    ) -> Derivation {
        Derivation {
            id: id.into(),
            premises,
            inference: format!("{id} follows"),
            conclusion: Conclusion {
                decision: decision.into(),
                answer: answer.into(),
            },
            status,
        }
    }

    /// The haiven-cli example from the design discussion.
    fn haiven() -> Graph {
        Graph::new([
            derivation(
                "drv-public",
                vec![
                    principle("open-by-default"),
                    fact("haiven-cli-has-no-secrets"),
                ],
                "dec-haiven-public",
                "haiven-cli is public and open source",
                DerivationStatus::OperatorConfirmed,
            ),
            derivation(
                "drv-downloads",
                vec![decision("dec-haiven-public")],
                "dec-downloads",
                "downloads come from public GitHub releases",
                DerivationStatus::AgentDerivedUnreviewed,
            ),
            derivation(
                "drv-owner",
                vec![decision("dec-haiven-public"), fact("btakita-not-org-admin")],
                "dec-who-publishes",
                "an org owner must make haiven-cli public",
                DerivationStatus::AgentDerivedUnreviewed,
            ),
        ])
        .unwrap()
    }

    #[test]
    fn why_walks_through_derived_premises_to_principles_and_facts() {
        let why = haiven().why("dec-who-publishes");
        assert!(why.supported());
        assert_eq!(why.steps.len(), 1);
        let step = &why.steps[0];
        assert_eq!(step.derivation, "drv-owner");
        let public = &step.premises[0];
        assert_eq!(public.node, decision("dec-haiven-public"));
        let because = public.because.as_ref().expect("derived premise recurses");
        assert_eq!(because.steps[0].derivation, "drv-public");
        assert_eq!(
            because.steps[0]
                .premises
                .iter()
                .map(|p| p.node.clone())
                .collect::<Vec<_>>(),
            vec![
                principle("open-by-default"),
                fact("haiven-cli-has-no-secrets")
            ]
        );
        assert!(
            because.steps[0]
                .premises
                .iter()
                .all(|p| p.because.is_none())
        );
        assert_eq!(step.premises[1].node, fact("btakita-not-org-admin"));
        assert!(step.premises[1].because.is_none());
    }

    #[test]
    fn a_decision_with_no_derivation_has_an_empty_unsupported_chain() {
        let why = haiven().why("dec-unknown");
        assert!(why.steps.is_empty());
        assert!(!why.supported());
    }

    #[test]
    fn rejecting_an_inference_step_keeps_the_step_visible_and_its_premises_standing() {
        let mut derivations: Vec<Derivation> = haiven().derivations.into_values().collect();
        for derivation in &mut derivations {
            if derivation.id == "drv-owner" {
                derivation.status = DerivationStatus::Rejected;
            }
        }
        let graph = Graph::new(derivations).unwrap();
        let why = graph.why("dec-who-publishes");
        assert_eq!(why.steps[0].status, DerivationStatus::Rejected);
        assert!(!why.supported(), "the only step was rejected");
        assert!(
            graph.why("dec-haiven-public").supported(),
            "its premise still stands"
        );
        let impacted: Vec<String> = graph
            .impact(&fact("btakita-not-org-admin"))
            .into_iter()
            .map(|impact| impact.derivation)
            .collect();
        assert!(
            impacted.is_empty(),
            "a rejected step does not propagate: {impacted:?}"
        );
    }

    #[test]
    fn a_changed_fact_flags_exactly_the_conclusions_that_used_it() {
        let impacts = haiven().impact(&fact("btakita-not-org-admin"));
        assert_eq!(
            impacts,
            vec![Impact {
                derivation: "drv-owner".into(),
                decision: "dec-who-publishes".into(),
                action: ImpactAction::Rederive,
                via: vec!["btakita-not-org-admin".into()],
            }]
        );
    }

    #[test]
    fn a_changed_principle_cascades_through_derived_decisions() {
        let impacts = haiven().impact(&principle("open-by-default"));
        let summary: Vec<(String, ImpactAction, Vec<String>)> = impacts
            .into_iter()
            .map(|impact| (impact.derivation, impact.action, impact.via))
            .collect();
        assert_eq!(
            summary,
            vec![
                (
                    "drv-public".into(),
                    ImpactAction::Flag,
                    vec!["open-by-default".into()]
                ),
                (
                    "drv-downloads".into(),
                    ImpactAction::Rederive,
                    vec!["open-by-default".into(), "dec-haiven-public".into()]
                ),
                (
                    "drv-owner".into(),
                    ImpactAction::Rederive,
                    vec!["open-by-default".into(), "dec-haiven-public".into()]
                ),
            ]
        );
    }

    #[test]
    fn a_principle_and_a_fact_with_one_id_are_different_premises() {
        let impacts = haiven().impact(&fact("open-by-default"));
        assert!(impacts.is_empty(), "{impacts:?}");
    }

    #[test]
    fn same_premises_reaching_different_answers_is_flagged() {
        let graph = Graph::new([
            derivation(
                "drv-a",
                vec![principle("velocity"), fact("release-blocked")],
                "dec-polish",
                "ship without the polish pass",
                DerivationStatus::AgentDerivedUnreviewed,
            ),
            derivation(
                "drv-b",
                vec![fact("release-blocked"), principle("velocity")],
                "dec-polish",
                "Do the polish pass first",
                DerivationStatus::AgentDerivedUnreviewed,
            ),
            derivation(
                "drv-c",
                vec![principle("velocity")],
                "dec-other",
                "x",
                DerivationStatus::AgentDerivedUnreviewed,
            ),
        ])
        .unwrap();
        assert_eq!(
            graph.contradictions(),
            vec![Contradiction {
                decision: "dec-polish".into(),
                kind: ContradictionKind::SamePremises,
                derivations: vec![
                    ("drv-a".into(), "ship without the polish pass".into()),
                    ("drv-b".into(), "Do the polish pass first".into()),
                ],
            }]
        );
    }

    #[test]
    fn different_premises_reaching_different_answers_is_a_premise_conflict() {
        let graph = Graph::new([
            derivation(
                "drv-a",
                vec![principle("velocity")],
                "dec-polish",
                "ship now",
                DerivationStatus::OperatorConfirmed,
            ),
            derivation(
                "drv-b",
                vec![principle("quality")],
                "dec-polish",
                "polish first",
                DerivationStatus::AgentDerivedUnreviewed,
            ),
            derivation(
                "drv-c",
                vec![principle("quality"), fact("x")],
                "dec-polish",
                "Ship   NOW",
                DerivationStatus::Rejected,
            ),
        ])
        .unwrap();
        let contradictions = graph.contradictions();
        assert_eq!(contradictions.len(), 1, "{contradictions:?}");
        assert_eq!(contradictions[0].kind, ContradictionKind::DifferentPremises);
        assert_eq!(
            contradictions[0]
                .derivations
                .iter()
                .map(|(id, _)| id.as_str())
                .collect::<Vec<_>>(),
            vec!["drv-a", "drv-b"],
            "a rejected derivation never contradicts"
        );
    }

    #[test]
    fn agreeing_derivations_differing_only_in_case_and_spacing_do_not_contradict() {
        let graph = Graph::new([
            derivation(
                "drv-a",
                vec![principle("p")],
                "dec",
                "Ship now",
                DerivationStatus::OperatorConfirmed,
            ),
            derivation(
                "drv-b",
                vec![fact("f")],
                "dec",
                " ship   now ",
                DerivationStatus::AgentDerivedUnreviewed,
            ),
        ])
        .unwrap();
        assert!(graph.contradictions().is_empty());
    }

    #[test]
    fn a_reasoning_cycle_terminates_and_is_marked() {
        let graph = Graph::new([
            derivation(
                "drv-a",
                vec![decision("dec-b")],
                "dec-a",
                "a",
                DerivationStatus::AgentDerivedUnreviewed,
            ),
            derivation(
                "drv-b",
                vec![decision("dec-a")],
                "dec-b",
                "b",
                DerivationStatus::AgentDerivedUnreviewed,
            ),
        ])
        .unwrap();
        let why = graph.why("dec-a");
        let inner = why.steps[0].premises[0].because.as_ref().unwrap();
        assert_eq!(inner.decision, "dec-b");
        assert!(inner.steps[0].premises[0].cycle);
        let impacts = graph.impact(&decision("dec-a"));
        assert_eq!(
            impacts.len(),
            2,
            "each derivation appears once: {impacts:?}"
        );
    }

    #[test]
    fn invalid_derivations_are_refused() {
        let ok = |id: &str| {
            derivation(
                id,
                vec![principle("p")],
                "dec",
                "a",
                DerivationStatus::OperatorConfirmed,
            )
        };
        assert_eq!(
            Graph::new([ok("d"), ok("d")]).unwrap_err(),
            GraphError::DuplicateId("d".into())
        );
        let mut empty = ok("e");
        empty.premises.clear();
        assert_eq!(
            Graph::new([empty]).unwrap_err(),
            GraphError::NoPremises("e".into())
        );
        let mut blank = ok("b");
        blank.inference = "  ".into();
        assert_eq!(
            Graph::new([blank]).unwrap_err(),
            GraphError::EmptyInference("b".into())
        );
        let selfish = derivation(
            "s",
            vec![decision("dec")],
            "dec",
            "a",
            DerivationStatus::OperatorConfirmed,
        );
        assert_eq!(
            Graph::new([selfish]).unwrap_err(),
            GraphError::SelfPremise("s".into())
        );
    }

    #[test]
    fn why_serializes_for_the_cycle_contract() {
        let value = serde_json::to_value(haiven().why("dec-downloads")).unwrap();
        assert_eq!(value["decision"], "dec-downloads");
        assert_eq!(value["steps"][0]["status"], "agent_derived_unreviewed");
        assert_eq!(value["steps"][0]["premises"][0]["node"]["kind"], "decision");
        assert_eq!(
            value["steps"][0]["premises"][0]["because"]["steps"][0]["status"],
            "operator_confirmed"
        );
        assert!(value["steps"][0]["premises"][0].get("cycle").is_none());
    }
}
