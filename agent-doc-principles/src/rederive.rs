//! Premise change detection (`#firstprinciplesrederive`).
//!
//! A derivation's conclusion is only as good as its premises. When the
//! operator edits or deletes a principle or a fact, every conclusion that
//! rested on it must be looked at again: an unreviewed agent derivation is
//! derived again from the current premises, an operator-confirmed one is
//! flagged for the operator and never silently rewritten
//! ([`crate::derivation::Graph::impact`]).
//!
//! This module is the pure half of that check:
//!
//! - [`facts_from_layers`] reads fact statements. A fact is an `agent:rules`
//!   item whose first line reads `fact [#id]: <statement>` (`fact` in any
//!   case, `#id` without brackets accepted, continuation lines extend the
//!   statement). Facts come from project and document layers only; the global
//!   principles file holds principles. A nearer fact with the same id replaces
//!   a farther one and lists the replaced source in `overrides`; a second fact
//!   with one id in the same file is a `fact_duplicate_id` issue and the first
//!   applies ([`compose_facts`]). A fact item stays in `agent_rules` as
//!   written. An item that starts like a fact but has no valid id or no
//!   statement is a `fact_malformed` issue ([`classify_fact`]): it stays an
//!   ordinary rule and is not a premise.
//! - [`PremiseRecord`] is one premise's statement plus the digest a caller
//!   stores (the caller supplies the hash function, so this crate stays free
//!   of hashing and storage). [`statement_digest_input`] is the canonical text
//!   that digest is taken over: whitespace runs collapse, so reflowing a
//!   statement is not a change; any other edit is.
//! - [`diff`] compares the stored records with the current ones: a premise
//!   whose digest differs is `changed`, one that is gone is `removed`. A new
//!   premise is not a change (nothing can have been derived from it before it
//!   existed), and a premise that only moved between files is not either.
//! - [`report`] turns the changes into the cycle-contract payload: each change
//!   with the conclusions it reaches, plus the graph's current contradictions.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::derivation::{Contradiction, Graph, Impact, NodeKind, NodeRef};
use crate::{
    ISSUE_FACT_DUPLICATE_ID, ISSUE_FACT_MALFORMED, Layer, Principle, PrincipleIssue,
    PrincipleScope, first_line, is_valid_id,
};

/// One premise statement in scope for a document: a principle or a fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PremiseRecord {
    pub kind: NodeKind,
    pub id: String,
    pub statement: String,
    /// Digest of [`statement_digest_input`] of `statement`.
    pub digest: String,
    /// The file the premise was read from.
    pub source: String,
}

impl PremiseRecord {
    /// A record whose digest is `hash(statement_digest_input(statement))`.
    pub fn new(
        kind: NodeKind,
        id: impl Into<String>,
        statement: impl Into<String>,
        source: impl Into<String>,
        hash: impl Fn(&str) -> String,
    ) -> Self {
        let statement = statement.into();
        Self {
            digest: hash(&statement_digest_input(&statement)),
            kind,
            id: id.into(),
            statement,
            source: source.into(),
        }
    }

    pub fn node(&self) -> NodeRef {
        NodeRef::new(self.kind, &self.id)
    }
}

/// The text a premise digest is taken over: whitespace runs collapse to one
/// space and the ends are trimmed. Case and punctuation are kept.
pub fn statement_digest_input(statement: &str) -> String {
    statement.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One fact as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedFact {
    pub id: String,
    pub statement: String,
}

/// How one rule text reads as a fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactForm {
    /// Not a fact: an ordinary rule.
    Plain,
    /// A well-formed fact.
    Fact(ParsedFact),
    /// Starts like a fact but is not one (`#factmalformedwarning`): the rule
    /// stays plain and is not a premise.
    Malformed { reason: String },
}

/// Classify one rule text as a fact.
///
/// The shape is `fact` (any case) followed by whitespace, `[`, `#`, or `:`,
/// then a `[#id]`, `#id`, `[id]`, or `:`. Anything else stays
/// [`FactForm::Plain`], so prose that opens with the word ("Fact checking
/// comes first.", "Facts matter.") is never reported.
pub fn classify_fact(rule: &str) -> FactForm {
    const NEEDS_ID: &str = "a fact needs an id: `fact [#id]: <statement>`";
    let malformed = |reason: &str| FactForm::Malformed {
        reason: reason.to_string(),
    };
    let mut lines = rule.lines();
    let Some(first) = lines.next() else {
        return FactForm::Plain;
    };
    let Some(after_keyword) = strip_fact_keyword(first.trim_start()) else {
        return FactForm::Plain;
    };
    let rest = after_keyword.trim_start();
    let (id, statement_head) = if let Some(bracketed) = rest.strip_prefix("[#") {
        let Some((id, tail)) = bracketed.split_once(']') else {
            return malformed("the `[#id]` has no closing `]`");
        };
        let Some(statement) = tail.trim_start().strip_prefix(':') else {
            return malformed("expected `:` after `[#id]`");
        };
        (id.trim(), statement)
    } else if let Some(hashed) = rest.strip_prefix('#') {
        let Some((id, statement)) = hashed.split_once(':') else {
            return malformed("expected `:` after `#id`");
        };
        (id.trim(), statement)
    } else if rest.starts_with(':') {
        return malformed(NEEDS_ID);
    } else if rest.starts_with('[') {
        return malformed("a fact id starts with `#`: `fact [#id]: <statement>`");
    } else {
        return FactForm::Plain;
    };
    if id.is_empty() {
        return malformed(NEEDS_ID);
    }
    if !is_valid_id(id) {
        return malformed(&format!(
            "fact id {id:?} must start with a letter or digit and may use only ASCII letters, digits, `-`, `_` and `.`"
        ));
    }
    let statement = std::iter::once(statement_head)
        .chain(lines)
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if statement.is_empty() {
        return malformed(&format!("fact [#{id}] has no statement"));
    }
    FactForm::Fact(ParsedFact {
        id: id.to_string(),
        statement,
    })
}

/// `fact` (any case) followed by whitespace, `[`, `#`, or `:`.
fn strip_fact_keyword(line: &str) -> Option<&str> {
    const KEYWORD: &str = "fact";
    let head = line.get(..KEYWORD.len())?;
    if !head.eq_ignore_ascii_case(KEYWORD) {
        return None;
    }
    let rest = &line[KEYWORD.len()..];
    let next = rest.chars().next()?;
    (next.is_whitespace() || matches!(next, '[' | '#' | ':')).then_some(rest)
}

/// Parse one rule text as a fact; `None` when it is not a well-formed
/// `fact [#id]: <statement>` item ([`classify_fact`] says why).
pub fn parse_fact(rule: &str) -> Option<ParsedFact> {
    match classify_fact(rule) {
        FactForm::Fact(parsed) => Some(parsed),
        FactForm::Plain | FactForm::Malformed { .. } => None,
    }
}

/// A fact in scope, after nearest-wins composition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Fact {
    pub id: String,
    pub statement: String,
    /// Where the fact is written: `project` or `document`.
    pub scope: PrincipleScope,
    /// The file the fact was read from.
    pub source: String,
    /// Sources of farther facts with this id that this one replaced, farthest
    /// first (`#firstprinciplesfactlist`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overrides: Vec<String>,
}

/// Facts in scope and the problems their authoring produced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FactComposition {
    pub facts: Vec<Fact>,
    pub issues: Vec<PrincipleIssue>,
}

/// Compose facts from `layers` (farthest first). The global layer is skipped
/// (the caller reports a fact written there). A nearer fact with the same id
/// replaces the farther one in place and records the replaced source in
/// [`Fact::overrides`]; within one layer the first declaration wins and each
/// later one is a [`ISSUE_FACT_DUPLICATE_ID`] issue. A fact-shaped rule that
/// is not a fact is a [`ISSUE_FACT_MALFORMED`] issue and composes nothing.
pub fn compose_facts(layers: &[Layer]) -> FactComposition {
    let mut composition = FactComposition::default();
    for layer in layers {
        if layer.scope == PrincipleScope::Global {
            continue;
        }
        let mut seen_in_layer: Vec<String> = Vec::new();
        for rule in &layer.rules {
            let parsed = match classify_fact(rule) {
                FactForm::Plain => continue,
                FactForm::Fact(parsed) => parsed,
                FactForm::Malformed { reason } => {
                    composition.issues.push(PrincipleIssue {
                        code: ISSUE_FACT_MALFORMED,
                        message: format!(
                            "{reason}; kept as an ordinary rule, not a premise: {:?}",
                            first_line(rule)
                        ),
                        source: layer.source.clone(),
                    });
                    continue;
                }
            };
            if seen_in_layer.contains(&parsed.id) {
                composition.issues.push(PrincipleIssue {
                    code: ISSUE_FACT_DUPLICATE_ID,
                    message: format!(
                        "fact [#{}] is declared twice in this file; the first one applies",
                        parsed.id
                    ),
                    source: layer.source.clone(),
                });
                continue;
            }
            seen_in_layer.push(parsed.id.clone());
            let fact = Fact {
                id: parsed.id,
                statement: parsed.statement,
                scope: layer.scope,
                source: layer.source.clone(),
                overrides: Vec::new(),
            };
            match composition
                .facts
                .iter_mut()
                .find(|existing| existing.id == fact.id)
            {
                Some(existing) => {
                    let mut overrides = std::mem::take(&mut existing.overrides);
                    overrides.push(existing.source.clone());
                    *existing = Fact { overrides, ..fact };
                }
                None => composition.facts.push(fact),
            }
        }
    }
    composition
}

/// Facts from `layers` (farthest first): [`compose_facts`] without its issues.
pub fn facts_from_layers(layers: &[Layer]) -> Vec<Fact> {
    compose_facts(layers).facts
}

/// Every principle and fact in scope as a record, principles first.
pub fn premise_records(
    principles: &[Principle],
    facts: &[Fact],
    hash: impl Fn(&str) -> String,
) -> Vec<PremiseRecord> {
    principles
        .iter()
        .map(|principle| {
            PremiseRecord::new(
                NodeKind::Principle,
                &principle.id,
                &principle.statement,
                &principle.source,
                &hash,
            )
        })
        .chain(facts.iter().map(|fact| {
            PremiseRecord::new(
                NodeKind::Fact,
                &fact.id,
                &fact.statement,
                &fact.source,
                &hash,
            )
        }))
        .collect()
}

/// How a premise changed since it was recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PremiseChangeKind {
    /// The statement now reads differently.
    Changed,
    /// The premise is no longer in any file in scope.
    Removed,
}

/// One premise whose statement changed or disappeared.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PremiseChange {
    pub premise: NodeRef,
    pub change: PremiseChangeKind,
    /// The statement as last recorded.
    pub previous_statement: String,
    /// The current statement; absent for a removed premise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub statement: Option<String>,
    /// Where the premise is written now, or was last written when removed.
    pub source: String,
}

/// Premises in `previous` that changed or disappeared in `current`, in
/// `(kind, id)` order.
pub fn diff(previous: &[PremiseRecord], current: &[PremiseRecord]) -> Vec<PremiseChange> {
    let current: BTreeMap<NodeRef, &PremiseRecord> = current
        .iter()
        .map(|record| (record.node(), record))
        .collect();
    let mut previous: Vec<&PremiseRecord> = previous.iter().collect();
    previous.sort_by_key(|record| record.node());
    previous
        .into_iter()
        .filter_map(|old| match current.get(&old.node()) {
            Some(now) if now.digest == old.digest => None,
            Some(now) => Some(PremiseChange {
                premise: old.node(),
                change: PremiseChangeKind::Changed,
                previous_statement: old.statement.clone(),
                statement: Some(now.statement.clone()),
                source: now.source.clone(),
            }),
            None => Some(PremiseChange {
                premise: old.node(),
                change: PremiseChangeKind::Removed,
                previous_statement: old.statement.clone(),
                statement: None,
                source: old.source.clone(),
            }),
        })
        .collect()
}

/// One changed premise and every conclusion it reaches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PremiseChangeReport {
    #[serde(flatten)]
    pub change: PremiseChange,
    /// Live derivations depending on the premise, directly or through a
    /// derived decision: `rederive` (unreviewed) or `flag` (confirmed).
    pub affected: Vec<Impact>,
}

/// What the cycle contract carries for premise changes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RederiveReport {
    pub premise_changes: Vec<PremiseChangeReport>,
    pub contradictions: Vec<Contradiction>,
}

impl RederiveReport {
    pub fn is_empty(&self) -> bool {
        self.premise_changes.is_empty() && self.contradictions.is_empty()
    }

    /// Guidance for the contract; `None` when there is nothing to act on.
    pub fn guidance(&self) -> Option<String> {
        (!self.is_empty()).then(|| PREMISE_CHANGES_GUIDANCE.to_string())
    }
}

/// How the agent acts on `premise_changes` and `derivation_contradictions`.
pub const PREMISE_CHANGES_GUIDANCE: &str = "\
premise_changes lists principles and facts whose statement changed or that \
were removed since the last turn, each with the conclusions that rested on \
it. For an affected conclusion with action `rederive` (an unreviewed agent \
derivation), derive it again from the current premises and state the new \
inference; if the answer changes, say what changed and why. For action `flag` \
(an operator-confirmed derivation), never rewrite it: show the operator the \
conclusion, the old and new premise, and ask whether it still holds. A \
removed premise supports nothing: re-derive or flag its conclusions the same \
way. derivation_contradictions lists live derivations of one decision that \
reach different answers: `same_premises` means an inference step is wrong, \
so re-check both inferences; `different_premises` means the premises \
conflict, so name them and ask the operator which one governs.";

/// The contract payload for `changes` over `graph`.
pub fn report(changes: Vec<PremiseChange>, graph: &Graph) -> RederiveReport {
    RederiveReport {
        premise_changes: changes
            .into_iter()
            .map(|change| PremiseChangeReport {
                affected: graph.impact(&change.premise),
                change,
            })
            .collect(),
        contradictions: graph.contradictions(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::derivation::{
        Conclusion, ContradictionKind, Derivation, DerivationStatus, ImpactAction,
    };

    fn hash(text: &str) -> String {
        format!("h({text})")
    }

    fn layer(scope: PrincipleScope, source: &str, rules: &[&str]) -> Layer {
        Layer {
            scope,
            source: source.into(),
            rules: rules.iter().map(|rule| rule.to_string()).collect(),
        }
    }

    fn record(kind: NodeKind, id: &str, statement: &str) -> PremiseRecord {
        PremiseRecord::new(kind, id, statement, "doc.md", hash)
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

    #[test]
    fn a_fact_item_reads_id_and_statement_in_both_id_forms() {
        assert_eq!(
            parse_fact("fact [#jws]: Credentials are JWS."),
            Some(ParsedFact {
                id: "jws".into(),
                statement: "Credentials are JWS.".into()
            })
        );
        assert_eq!(
            parse_fact("FACT #not-admin:\n  btakita is not\n  an org admin."),
            Some(ParsedFact {
                id: "not-admin".into(),
                statement: "btakita is not\nan org admin.".into()
            })
        );
    }

    #[test]
    fn rules_that_only_look_like_facts_are_not_facts() {
        for rule in [
            "Facts matter more than opinions.",
            "fact: no id here",
            "fact [#bad id]: spaces in the id",
            "fact [#empty]:   ",
            "factor [#x]: a different word",
            "fact [#open: no closing bracket",
        ] {
            assert_eq!(parse_fact(rule), None, "{rule:?}");
        }
    }

    /// `#factmalformedwarning`: every shape `parse_fact` refuses for a bad id
    /// or an empty statement says why, so nothing the operator meant as a
    /// premise disappears silently.
    #[test]
    fn a_fact_shaped_rule_that_is_not_a_fact_says_why() {
        for (rule, needle) in [
            ("fact: no id here", "needs an id"),
            ("FACT: no id here", "needs an id"),
            ("fact [#]: empty id", "needs an id"),
            ("fact #: empty id", "needs an id"),
            ("fact [jws]: no hash", "starts with `#`"),
            ("fact [#open: unclosed", "closing `]`"),
            ("fact [#ok] missing colon", "expected `:`"),
            ("fact #nocolon", "expected `:`"),
            ("fact [#has space]: x", "may use only"),
            ("fact #has space: x", "may use only"),
            ("fact [#-lead]: x", "start with a letter or digit"),
            ("fact [#empty]:", "has no statement"),
            ("fact [#empty]:   \n   \n", "has no statement"),
            ("fact #empty:", "has no statement"),
        ] {
            match classify_fact(rule) {
                FactForm::Malformed { reason } => {
                    assert!(reason.contains(needle), "{rule:?}: {reason}");
                }
                other => panic!("{rule:?} should be malformed, got {other:?}"),
            }
            assert_eq!(parse_fact(rule), None, "{rule:?}");
        }
    }

    /// A warning on prose would teach the operator to ignore the warning.
    #[test]
    fn prose_that_opens_with_the_word_fact_is_plain_and_never_reported() {
        for rule in [
            "",
            "fact",
            "Facts matter more than opinions.",
            "Fact checking comes before the merge.",
            "fact-check the release notes",
            "factor [#x]: a different word",
            "In fact [#x]: not at the start",
        ] {
            assert_eq!(classify_fact(rule), FactForm::Plain, "{rule:?}");
        }
    }

    #[test]
    fn every_well_formed_fact_still_classifies_as_a_fact() {
        for rule in [
            "fact [#jws]: Credentials are JWS.",
            "fact[#jws]: no space",
            "fact#jws: no space",
            "  fact [# jws ] : spaced",
            "fact [#jws]:\n  statement on the next line",
        ] {
            assert!(
                matches!(classify_fact(rule), FactForm::Fact(_)),
                "{rule:?}: {:?}",
                classify_fact(rule)
            );
        }
    }

    #[test]
    fn a_malformed_fact_is_reported_at_its_file_and_is_not_a_premise() {
        let layers = [layer(
            PrincipleScope::Document,
            "doc.md",
            &["fact [#bad id]: not a premise", "fact [#ok]: a premise"],
        )];
        let composition = compose_facts(&layers);
        assert_eq!(
            composition
                .facts
                .iter()
                .map(|fact| fact.id.as_str())
                .collect::<Vec<_>>(),
            vec!["ok"]
        );
        assert_eq!(composition.issues.len(), 1, "{:?}", composition.issues);
        let issue = &composition.issues[0];
        assert_eq!(
            (issue.code, issue.source.as_str()),
            (ISSUE_FACT_MALFORMED, "doc.md")
        );
        assert!(
            issue
                .message
                .ends_with("not a premise: \"fact [#bad id]: not a premise\""),
            "{}",
            issue.message
        );
    }

    #[test]
    fn nearer_facts_replace_farther_ones_and_the_global_layer_holds_none() {
        let facts = facts_from_layers(&[
            layer(
                PrincipleScope::Global,
                "global.md",
                &["fact [#g]: ignored here"],
            ),
            layer(
                PrincipleScope::Project,
                "project.md",
                &["fact [#jws]: Credentials are JWS.", "fact [#os]: Linux."],
            ),
            layer(
                PrincipleScope::Document,
                "doc.md",
                &[
                    "fact [#os]: Linux and macOS.",
                    "fact [#os]: a second declaration is ignored",
                    "Run the focused test first.",
                ],
            ),
        ]);
        assert_eq!(
            facts,
            vec![
                Fact {
                    id: "jws".into(),
                    statement: "Credentials are JWS.".into(),
                    scope: PrincipleScope::Project,
                    source: "project.md".into(),
                    overrides: vec![],
                },
                Fact {
                    id: "os".into(),
                    statement: "Linux and macOS.".into(),
                    scope: PrincipleScope::Document,
                    source: "doc.md".into(),
                    overrides: vec!["project.md".into()],
                },
            ]
        );
    }

    #[test]
    fn fact_composition_chains_overrides_and_reports_duplicates_in_one_file() {
        let composed = compose_facts(&[
            layer(
                PrincipleScope::Project,
                "outer/.agent-doc/project.md",
                &["fact [#os]: Linux."],
            ),
            layer(
                PrincipleScope::Project,
                "outer/inner/.agent-doc/project.md",
                &[
                    "fact #os: Linux and macOS.",
                    "fact [#os]: second in this file",
                ],
            ),
            layer(
                PrincipleScope::Document,
                "doc.md",
                &["FACT [#os]: Linux, macOS, Windows."],
            ),
        ]);
        assert_eq!(composed.facts.len(), 1);
        assert_eq!(composed.facts[0].statement, "Linux, macOS, Windows.");
        assert_eq!(composed.facts[0].scope, PrincipleScope::Document);
        assert_eq!(
            composed.facts[0].overrides,
            vec![
                "outer/.agent-doc/project.md".to_string(),
                "outer/inner/.agent-doc/project.md".to_string()
            ]
        );
        assert_eq!(
            composed.issues,
            vec![PrincipleIssue {
                code: ISSUE_FACT_DUPLICATE_ID,
                message: "fact [#os] is declared twice in this file; the first one applies".into(),
                source: "outer/inner/.agent-doc/project.md".into(),
            }]
        );
        assert_eq!(facts_from_layers(&[]), Vec::<Fact>::new());
    }

    #[test]
    fn reflowing_a_statement_keeps_its_digest_and_rewording_changes_it() {
        let base = record(NodeKind::Principle, "p", "Velocity over polish.");
        let reflowed = record(NodeKind::Principle, "p", "  Velocity\n  over   polish. ");
        let reworded = record(NodeKind::Principle, "p", "Polish over velocity.");
        let recased = record(NodeKind::Principle, "p", "velocity over polish.");
        assert_eq!(base.digest, reflowed.digest);
        assert_ne!(base.digest, reworded.digest);
        assert_ne!(base.digest, recased.digest, "case is part of the statement");
    }

    #[test]
    fn diff_reports_changed_and_removed_premises_but_not_new_or_moved_ones() {
        let previous = vec![
            record(NodeKind::Principle, "velocity", "Velocity over polish."),
            record(NodeKind::Fact, "jws", "Credentials are JWS."),
            record(NodeKind::Fact, "stable", "Unchanged."),
            record(NodeKind::Principle, "moved", "Same words."),
        ];
        let mut moved = record(NodeKind::Principle, "moved", "Same words.");
        moved.source = "project.md".into();
        let current = vec![
            record(
                NodeKind::Principle,
                "velocity",
                "Correctness over velocity.",
            ),
            record(NodeKind::Fact, "stable", "Unchanged."),
            record(NodeKind::Fact, "new", "Brand new."),
            moved,
        ];
        assert_eq!(
            diff(&previous, &current),
            vec![
                PremiseChange {
                    premise: NodeRef::new(NodeKind::Principle, "velocity"),
                    change: PremiseChangeKind::Changed,
                    previous_statement: "Velocity over polish.".into(),
                    statement: Some("Correctness over velocity.".into()),
                    source: "doc.md".into(),
                },
                PremiseChange {
                    premise: NodeRef::new(NodeKind::Fact, "jws"),
                    change: PremiseChangeKind::Removed,
                    previous_statement: "Credentials are JWS.".into(),
                    statement: None,
                    source: "doc.md".into(),
                },
            ]
        );
    }

    #[test]
    fn a_principle_and_a_fact_sharing_an_id_are_tracked_separately() {
        let previous = vec![
            record(NodeKind::Principle, "x", "principle text"),
            record(NodeKind::Fact, "x", "fact text"),
        ];
        let current = vec![
            record(NodeKind::Principle, "x", "principle text"),
            record(NodeKind::Fact, "x", "fact text, edited"),
        ];
        let changes = diff(&previous, &current);
        assert_eq!(changes.len(), 1, "{changes:?}");
        assert_eq!(changes[0].premise, NodeRef::new(NodeKind::Fact, "x"));
    }

    #[test]
    fn report_attaches_rederive_and_flag_actions_and_the_graph_contradictions() {
        let graph = Graph::new([
            derivation(
                "drv-public",
                vec![NodeRef::new(NodeKind::Principle, "open-by-default")],
                "dec-public",
                "public",
                DerivationStatus::OperatorConfirmed,
            ),
            derivation(
                "drv-downloads",
                vec![NodeRef::new(NodeKind::Decision, "dec-public")],
                "dec-downloads",
                "GitHub releases",
                DerivationStatus::AgentDerivedUnreviewed,
            ),
            derivation(
                "drv-owner",
                vec![NodeRef::new(NodeKind::Fact, "not-admin")],
                "dec-owner",
                "an org owner publishes",
                DerivationStatus::AgentDerivedUnreviewed,
            ),
            derivation(
                "drv-owner-2",
                vec![NodeRef::new(NodeKind::Fact, "is-admin")],
                "dec-owner",
                "btakita publishes",
                DerivationStatus::AgentDerivedUnreviewed,
            ),
        ])
        .unwrap();
        let changes = diff(
            &[
                record(NodeKind::Principle, "open-by-default", "Open by default."),
                record(NodeKind::Fact, "not-admin", "btakita is not an org admin."),
            ],
            &[record(
                NodeKind::Principle,
                "open-by-default",
                "Closed by default.",
            )],
        );
        let report = report(changes, &graph);
        let summary: Vec<(String, Vec<(String, ImpactAction)>)> = report
            .premise_changes
            .iter()
            .map(|change| {
                (
                    change.change.premise.id.clone(),
                    change
                        .affected
                        .iter()
                        .map(|impact| (impact.derivation.clone(), impact.action))
                        .collect(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                (
                    "open-by-default".into(),
                    vec![
                        ("drv-public".into(), ImpactAction::Flag),
                        ("drv-downloads".into(), ImpactAction::Rederive),
                    ]
                ),
                (
                    "not-admin".into(),
                    vec![("drv-owner".into(), ImpactAction::Rederive)]
                ),
            ]
        );
        assert_eq!(report.contradictions.len(), 1);
        assert_eq!(
            report.contradictions[0].kind,
            ContradictionKind::DifferentPremises
        );
        assert!(report.guidance().is_some());
    }

    #[test]
    fn an_unchanged_graph_without_contradictions_reports_nothing() {
        let report = report(Vec::new(), &Graph::default());
        assert!(report.is_empty());
        assert_eq!(report.guidance(), None);
    }

    #[test]
    fn a_change_report_serializes_flat_for_the_cycle_contract() {
        let change = PremiseChangeReport {
            change: PremiseChange {
                premise: NodeRef::new(NodeKind::Fact, "jws"),
                change: PremiseChangeKind::Removed,
                previous_statement: "Credentials are JWS.".into(),
                statement: None,
                source: "doc.md".into(),
            },
            affected: Vec::new(),
        };
        let value = serde_json::to_value(&change).unwrap();
        assert_eq!(value["premise"]["kind"], "fact");
        assert_eq!(value["premise"]["id"], "jws");
        assert_eq!(value["change"], "removed");
        assert_eq!(value["previous_statement"], "Credentials are JWS.");
        assert!(value.get("statement").is_none());
        assert_eq!(value["affected"], serde_json::json!([]));
    }
}
