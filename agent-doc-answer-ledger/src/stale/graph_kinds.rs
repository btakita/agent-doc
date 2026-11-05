//! Stale kinds that need decision-graph edges (`#decisionstalegraph`).
//!
//! [`super`] derives the stale kinds the answer ledger settles on its own.
//! The three kinds here read the typed [`DecisionGraph`] projected from the
//! same ledger, and are flag-only: the binary never closes them.
//!
//! - [`StaleKind::MootByDecision`]: the item assumes a choice the latest
//!   delivered answer of a decision linked to it rejects. Either the answer
//!   turns the work down (it leads with `no`, `moot`, `not needed`, `drop it`,
//!   …), or the decision chose between options and the item names only an
//!   option the answer did not choose.
//! - [`StaleKind::BuiltOnSuperseded`]: the item `applies_to` or `depends_on`
//!   a decision (directly, by citing one of its copies, or transitively
//!   through `depends_on`) whose answer a later delivered revision
//!   superseded. The dependents come from [`DecisionGraph::impact`] of the
//!   superseding revision.
//! - [`StaleKind::ContradictsDecision`]: the item is linked `conflicts_with`
//!   an answered decision, or it restates an answer a later revision of a
//!   decision superseded (and not the current answer).
//!
//! An item is reconciled, and no longer flagged, once its text carries the
//! current revision id the finding cites (`<decision>@<hash>`), the handle the
//! suggested `--*-edit` rewrite keeps.

use std::collections::{BTreeMap, BTreeSet};

use super::{StaleKind, StaleLink, StaleTrackedItem, asks_question, delivered};
use crate::extract::{
    Observations, TrackedComponent, TrackedItemObs, bracket_ids, is_question_text,
};
use crate::graph::{
    DecisionGraph, EdgeSource, GraphEdge, ImpactAction, Node, NodeKind, touches_clause,
};
use crate::{Decision, EdgeKind, Ledger, Revision, RevisionRelation, id_key, text};

/// Phrases (over normalized tokens, so `won't` is `wont`) that turn the work a
/// decision asked about down when they lead the answer.
const REJECTION_PHRASES: &[&str] = &[
    "moot",
    "not needed",
    "no longer needed",
    "unnecessary",
    "obsolete",
    "wont do",
    "wontfix",
    "wont fix",
    "drop it",
    "drop this",
    "drop that",
    "skip it",
    "skip this",
    "skip that",
    "cancel it",
    "cancel this",
    "abandon",
    "abandoned",
    "not doing",
    "dont do",
    "do not do",
    "no need",
];

/// Words that make a question negative (`Should we skip the cache?`): a bare
/// `No` to it keeps the work, so it is not read as a rejection.
const NEGATIVE_QUESTION_WORDS: &[&str] = &[
    "not", "no", "dont", "skip", "without", "never", "stop", "drop", "remove", "cancel",
];

/// Words that negate an option the answer names (`Friday, not Monday`).
const OPTION_NEGATIONS: &[&str] = &["not", "no", "never", "dont", "without", "instead"];

/// The answer's first clause, as normalized tokens.
fn leading_clause(answer: &str) -> Vec<String> {
    let end = answer
        .find(['.', ';', ',', ':', '!', '\n', '—', '('])
        .unwrap_or(answer.len());
    text::tokens(&answer[..end])
}

/// Whether `answer` turns down the work `question` asked about.
pub fn rejects_work(question: &str, answer: &str) -> bool {
    if is_question_text(answer) {
        return false;
    }
    let lead = leading_clause(answer);
    let Some(first) = lead.first() else {
        return false;
    };
    if matches!(first.as_str(), "no" | "nope") {
        let negative = text::tokens(&text::question_body(question))
            .iter()
            .any(|word| NEGATIVE_QUESTION_WORDS.contains(&word.as_str()));
        return !negative;
    }
    let padded = format!(" {} ", lead.join(" "));
    REJECTION_PHRASES
        .iter()
        .any(|phrase| padded.contains(&format!(" {phrase} ")))
}

/// The options a choice question offers: a parenthesized `(a or b)` set, or
/// the `a, b, or c` list after the last `:` of a choose/decide question.
pub fn question_options(question: &str) -> Vec<String> {
    let body = text::question_body(question);
    if let Some(set) = text::anchors(&body)
        .into_iter()
        .find_map(|anchor| anchor.strip_prefix("opts:").map(str::to_string))
    {
        return set.split('|').map(str::to_string).collect();
    }
    if !matches!(text::decision_kind(&body), Some("choose" | "decide")) {
        return Vec::new();
    }
    let Some((_, list)) = body.rsplit_once(':') else {
        return Vec::new();
    };
    let lower = list.to_lowercase();
    if !lower.contains(" or ") {
        return Vec::new();
    }
    let options: Vec<String> = lower
        .split(',')
        .flat_map(|part| part.split(" or "))
        .map(|option| text::tokens(option.trim().trim_start_matches("or ")).join(" "))
        .filter(|option| !text::content_tokens(option).is_empty())
        .collect();
    if options.len() >= 2 {
        options
    } else {
        Vec::new()
    }
}

/// How `answer` treats `option`: `None` when it does not name it, else
/// whether the mention is negated.
fn option_mention(words: &[String], option: &str) -> Option<bool> {
    let wanted = text::content_tokens(option);
    if wanted.is_empty() || !wanted.iter().all(|token| words.contains(token)) {
        return None;
    }
    let first = words.iter().position(|word| wanted.contains(word))?;
    Some(
        words[first.saturating_sub(3)..first]
            .iter()
            .any(|word| OPTION_NEGATIONS.contains(&word.as_str())),
    )
}

/// The options of `question` that `answer` rejects: every option it negates,
/// and, once it chooses at least one, every option it did not choose. Empty
/// when the answer chooses nothing and negates nothing.
pub fn rejected_options(question: &str, answer: &str) -> (Vec<String>, Vec<String>) {
    let options = question_options(question);
    let words = text::tokens(answer);
    let mut chosen = Vec::new();
    let mut rejected = Vec::new();
    let mut unnamed = Vec::new();
    for option in options {
        match option_mention(&words, &option) {
            Some(false) => chosen.push(option),
            Some(true) => rejected.push(option),
            None => unnamed.push(option),
        }
    }
    if !chosen.is_empty() {
        rejected.extend(unnamed);
    }
    (chosen, rejected)
}

fn names_option(item_tokens: &BTreeSet<String>, option: &str) -> bool {
    let wanted = text::content_tokens(option);
    !wanted.is_empty() && wanted.is_subset(item_tokens)
}

/// Whether `item` restates `answer`: most of the answer's content tokens
/// (and every number it names) are in the item.
pub fn restates_answer(answer: &str, item: &str) -> bool {
    let said = text::content_tokens(answer);
    if said.len() < 2 {
        return false;
    }
    let item = text::content_tokens(item);
    let numbers_kept = said
        .iter()
        .filter(|token| token.chars().any(|c| c.is_ascii_digit()))
        .all(|token| item.contains(token));
    let shared = said.intersection(&item).count() as f64;
    numbers_kept && shared / said.len() as f64 >= 0.6
}

/// The index of the latest revision that supersedes its predecessor, when it
/// is delivered.
fn last_supersede(decision: &Decision) -> Option<usize> {
    let index = decision
        .revisions
        .iter()
        .rposition(|revision| revision.relation == RevisionRelation::Supersedes)?;
    (index > 0 && delivered(&decision.revisions[index])).then_some(index)
}

/// One way an item reaches a decision.
#[derive(Debug, Clone)]
struct Link<'a> {
    decision: &'a Decision,
    link: StaleLink,
    /// The decision only asks about the item (a request raised for it), so
    /// it may settle a side question rather than the item's premise.
    asked_about: bool,
}

/// A dependent the impact walk of a superseding revision flagged.
#[derive(Debug, Clone)]
struct SupersededDependent {
    decision: String,
    relation: EdgeKind,
}

/// The decision graph plus the impact of every delivered supersede, built once
/// per stale pass.
pub(super) struct GraphStale<'a> {
    ledger: &'a Ledger,
    graph: DecisionGraph,
    superseded: BTreeMap<String, Vec<SupersededDependent>>,
}

impl<'a> GraphStale<'a> {
    pub(super) fn new(ledger: &'a Ledger, observations: &Observations) -> Self {
        let mut graph = DecisionGraph::build(ledger, observations);
        add_citations(&mut graph, observations);
        let mut superseded: BTreeMap<String, Vec<SupersededDependent>> = BTreeMap::new();
        for decision in &ledger.decisions {
            let Some(index) = last_supersede(decision) else {
                continue;
            };
            let Some(impact) = graph.impact(&decision.id, Some(&decision.revisions[index].id))
            else {
                continue;
            };
            for row in impact.flagged {
                if row.action == ImpactAction::Flag
                    && matches!(row.relation, EdgeKind::AppliesTo | EdgeKind::DependsOn)
                {
                    superseded
                        .entry(row.node)
                        .or_default()
                        .push(SupersededDependent {
                            decision: decision.id.clone(),
                            relation: row.relation,
                        });
                }
            }
        }
        Self {
            ledger,
            graph,
            superseded,
        }
    }

    fn decision(&self, id: &str) -> Option<&'a Decision> {
        let ledger: &'a Ledger = self.ledger;
        ledger
            .decisions
            .iter()
            .find(|decision| decision.id == id)
            .filter(|decision| !decision.revisions.is_empty())
    }

    /// The item's own decision (it is a copy of the question), if any.
    fn own_decision(&self, item: &TrackedItemObs) -> Option<String> {
        self.graph.resolve_decision(&item.id).or_else(|| {
            self.ledger
                .decision_for_key(&id_key(&item.id))
                .map(|decision| decision.id.clone())
        })
    }

    /// A node id resolved to the decision it belongs to.
    fn decision_of_node(&self, id: &str) -> Option<String> {
        match self.graph.nodes.get(id) {
            Some(node) if node.kind == NodeKind::Decision => Some(node.id.clone()),
            _ => self.graph.resolve_decision(id),
        }
    }

    /// Every answered decision linked to `item`: identity, `applies_to`
    /// either way (citations included), and `depends_on` from the item or its
    /// own decision.
    fn links(&self, item: &TrackedItemObs) -> Vec<Link<'a>> {
        let own = self.own_decision(item);
        let mut out: Vec<Link<'a>> = Vec::new();
        let push =
            |id: Option<String>, link: StaleLink, asked_about: bool, out: &mut Vec<Link<'a>>| {
                let Some(decision) = id.as_deref().and_then(|id| self.decision(id)) else {
                    return;
                };
                if link != StaleLink::Identity && own.as_deref() == Some(decision.id.as_str()) {
                    return;
                }
                if !out.iter().any(|seen| seen.decision.id == decision.id) {
                    out.push(Link {
                        decision,
                        link,
                        asked_about,
                    });
                }
            };
        push(own.clone(), StaleLink::Identity, false, &mut out);
        let members: Vec<String> = std::iter::once(item.id.clone())
            .chain(own.iter().cloned())
            .collect();
        // The item cites the decision, or says it depends on it: its premise.
        for member in &members {
            let cited: Vec<String> = self
                .graph
                .outgoing(member, EdgeKind::AppliesTo)
                .map(|edge| edge.to.clone())
                .collect();
            for target in cited {
                let decision = self.graph.resolve_decision(&target);
                push(decision, StaleLink::AppliesTo, false, &mut out);
            }
        }
        for member in &members {
            let targets: Vec<String> = self
                .graph
                .outgoing(member, EdgeKind::DependsOn)
                .map(|edge| edge.to.clone())
                .collect();
            for target in targets {
                let decision = self.decision_of_node(&target);
                push(decision, StaleLink::DependsOn, false, &mut out);
            }
        }
        // A request raised for the item concerns it.
        for member in &members {
            let askers: Vec<String> = self
                .graph
                .incoming(member, EdgeKind::AppliesTo)
                .map(|edge| edge.from.clone())
                .collect();
            for asker in askers {
                let decision = self.decision_of_node(&asker);
                push(decision, StaleLink::AppliesTo, true, &mut out);
            }
        }
        out
    }

    /// The first graph-derived stale finding for an open item: moot first,
    /// then a contradiction, then a superseded premise.
    pub(super) fn stale(&self, item: &TrackedItemObs) -> Option<StaleTrackedItem> {
        let links = self.links(item);
        self.moot_by_decision(item, &links)
            .or_else(|| self.contradicts_decision(item, &links))
            .or_else(|| self.built_on_superseded(item))
    }

    fn moot_by_decision(
        &self,
        item: &TrackedItemObs,
        links: &[Link<'a>],
    ) -> Option<StaleTrackedItem> {
        let item_tokens = text::content_tokens(&item.text);
        for Link {
            decision,
            link,
            asked_about,
        } in links
        {
            let Some(latest) = decision.latest() else {
                continue;
            };
            if !delivered(latest) || is_question_text(&latest.answer) || reconciled(item, latest) {
                continue;
            }
            // A question copy that is answered is `answered_but_open`; only
            // work rides on the choice.
            if *link == StaleLink::Identity && asks_question(item) {
                continue;
            }
            let (chosen, rejected) = rejected_options(&decision.question, &latest.answer);
            let named_rejected: Vec<&String> = rejected
                .iter()
                .filter(|option| names_option(&item_tokens, option))
                .collect();
            let premise = if !named_rejected.is_empty()
                && !chosen
                    .iter()
                    .any(|option| names_option(&item_tokens, option))
            {
                Premise::Option(
                    named_rejected
                        .iter()
                        .map(|option| option.to_string())
                        .collect(),
                )
            } else if rejects_work(&decision.question, &latest.answer)
                // A request about the item may turn down a side question
                // (`also add tests?`); it is the item's premise only when it
                // asks about the item's own subject.
                && (!asked_about || touches_clause(&decision.question, &item.text))
            {
                Premise::Work
            } else {
                continue;
            };
            let evidence = match &premise {
                Premise::Option(options) => options
                    .iter()
                    .map(|option| format!("option:{option}"))
                    .collect(),
                Premise::Work => Vec::new(),
            };
            let why = match &premise {
                Premise::Option(options) => format!(
                    "assumes {}, which {} rejects",
                    options
                        .iter()
                        .map(|option| format!("‘{option}’"))
                        .collect::<Vec<_>>()
                        .join(", "),
                    cite(decision, latest)
                ),
                Premise::Work => format!("is work that {} turns down", cite(decision, latest)),
            };
            return Some(finding(
                StaleKind::MootByDecision,
                item,
                decision,
                latest,
                *link,
                evidence,
                why,
            ));
        }
        None
    }

    fn contradicts_decision(
        &self,
        item: &TrackedItemObs,
        links: &[Link<'a>],
    ) -> Option<StaleTrackedItem> {
        let own = self.own_decision(item);
        let members: Vec<String> = std::iter::once(item.id.clone())
            .chain(own.iter().cloned())
            .collect();
        // An operator `conflicts with [#id]` link, either way.
        let mut conflicting: Vec<String> = Vec::new();
        for member in &members {
            conflicting.extend(
                self.graph
                    .outgoing(member, EdgeKind::ConflictsWith)
                    .map(|edge| edge.to.clone()),
            );
            conflicting.extend(
                self.graph
                    .incoming(member, EdgeKind::ConflictsWith)
                    .map(|edge| edge.from.clone()),
            );
        }
        for other in conflicting {
            let Some(decision) = self
                .decision_of_node(&other)
                .and_then(|id| self.decision(&id))
            else {
                continue;
            };
            if own.as_deref() == Some(decision.id.as_str()) {
                continue;
            }
            let Some(latest) = decision.latest() else {
                continue;
            };
            if !delivered(latest) || is_question_text(&latest.answer) || reconciled(item, latest) {
                continue;
            }
            let why = format!("is linked `conflicts_with` {}", cite(decision, latest));
            return Some(finding(
                StaleKind::ContradictsDecision,
                item,
                decision,
                latest,
                StaleLink::ConflictsWith,
                Vec::new(),
                why,
            ));
        }
        // Restating an answer a later revision superseded: a linked decision,
        // or any decision whose superseded answer the item repeats nearly word
        // for word.
        let linked: BTreeMap<&str, StaleLink> = links
            .iter()
            .filter(|link| link.link != StaleLink::Identity)
            .map(|link| (link.decision.id.as_str(), link.link))
            .collect();
        for decision in &self.ledger.decisions {
            if own.as_deref() == Some(decision.id.as_str()) {
                continue;
            }
            let Some(index) = last_supersede(decision) else {
                continue;
            };
            let Some(latest) = decision.latest() else {
                continue;
            };
            if !delivered(latest) || reconciled(item, latest) {
                continue;
            }
            let link = linked.get(decision.id.as_str()).copied();
            let restated = decision.revisions[..index].iter().rev().find(|old| {
                if link.is_some() {
                    restates_answer(&old.answer, &item.text)
                } else {
                    text::similar_questions(&old.answer, &item.text).is_some()
                }
            });
            let Some(old) = restated else { continue };
            if restates_answer(&latest.answer, &item.text) {
                continue;
            }
            let why = format!(
                "restates “{}” ({}), an answer {} superseded",
                old.answer,
                old.id,
                cite(decision, latest)
            );
            return Some(finding(
                StaleKind::ContradictsDecision,
                item,
                decision,
                latest,
                link.unwrap_or(StaleLink::Restates),
                vec![old.id.clone()],
                why,
            ));
        }
        None
    }

    fn built_on_superseded(&self, item: &TrackedItemObs) -> Option<StaleTrackedItem> {
        let own = self.own_decision(item);
        let dependents = self
            .superseded
            .get(&item.id)
            .into_iter()
            .chain(own.as_ref().and_then(|own| self.superseded.get(own)))
            .flatten();
        let item_first_seen = self.ledger.key_first_seen.get(&id_key(&item.id)).copied();
        for dependent in dependents {
            if own.as_deref() == Some(dependent.decision.as_str()) {
                continue;
            }
            let Some(decision) = self.decision(&dependent.decision) else {
                continue;
            };
            let (Some(index), Some(latest)) = (last_supersede(decision), decision.latest()) else {
                continue;
            };
            if reconciled(item, latest) {
                continue;
            }
            let change = &decision.revisions[index];
            // Written after the change: it was built on the current answer.
            if let (Some(item_seen), Some(change_seen)) = (item_first_seen, change.first_seen_at)
                && item_seen >= change_seen
            {
                continue;
            }
            let old = &decision.revisions[index - 1];
            let (link, verb) = if dependent.relation == EdgeKind::DependsOn {
                (StaleLink::DependsOn, "depends on")
            } else {
                (StaleLink::AppliesTo, "applies to")
            };
            let why = format!(
                "{verb} {}, whose earlier answer “{}” ({}) a later revision superseded",
                cite(decision, latest),
                old.answer,
                old.id
            );
            return Some(finding(
                StaleKind::BuiltOnSuperseded,
                item,
                decision,
                latest,
                link,
                vec![old.id.clone()],
                why,
            ));
        }
        None
    }
}

enum Premise {
    Work,
    Option(Vec<String>),
}

/// The item already names the revision the finding cites: it was rewritten
/// against the current answer.
fn reconciled(item: &TrackedItemObs, latest: &Revision) -> bool {
    item.text
        .to_ascii_lowercase()
        .contains(&latest.id.to_ascii_lowercase())
}

fn cite(decision: &Decision, revision: &Revision) -> String {
    format!(
        "decision `{}` (“{}”, {})",
        decision.id, revision.answer, revision.location
    )
}

fn finding(
    kind: StaleKind,
    item: &TrackedItemObs,
    decision: &Decision,
    revision: &Revision,
    link: StaleLink,
    evidence: Vec<String>,
    why: String,
) -> StaleTrackedItem {
    let action = graph_action(item, decision, revision, &why);
    StaleTrackedItem {
        kind,
        component: item.component,
        item: item.id.clone(),
        item_text: text::question_body(&item.text),
        decision_id: decision.id.clone(),
        revision_id: revision.id.clone(),
        answer: revision.answer.clone(),
        location: revision.location.clone(),
        link,
        evidence,
        auto_close: false,
        action,
    }
}

fn graph_action(
    item: &TrackedItemObs,
    decision: &Decision,
    revision: &Revision,
    why: &str,
) -> String {
    let id = &item.id;
    let component = item.component.as_str();
    let dec = &decision.id;
    let rev = &revision.id;
    let what = match item.component {
        TrackedComponent::Todo => format!(
            "If the decision leaves nothing to do, close it with --todo-done {id} citing `{dec}`; otherwise rewrite it against the current answer with --todo-edit '{id}=<action> ({rev})'."
        ),
        TrackedComponent::Backlog => format!(
            "If the decision leaves nothing to do, complete it with --done {id} citing `{dec}`; otherwise rewrite it against the current answer with --backlog-edit '{id}=<action> ({rev})'."
        ),
        TrackedComponent::Review => format!(
            "If the decision settles the gate, resolve it with --review-resolve {id} citing `{dec}`; otherwise record what still holds with --review-edit '{id}=<text> ({rev})'."
        ),
        TrackedComponent::Icebox => format!(
            "Parked work is never closed for you: read the decision and act on it (complete it with --done {id} citing `{dec}`, or record why it stays parked with --icebox-edit '{id}=<text> ({rev})')."
        ),
    };
    format!(
        "Open agent:{component} item [#{id}] {why}. {what} Keeping `{rev}` in the text marks it reconciled. (#decisionstalegraph)"
    )
}

/// Tracked items that cite a decision copy (`… (source: [#y2fn])`) apply to
/// it, as a question row's citation already does; add the edges the ledger
/// does not carry for backlog, review and icebox items.
fn add_citations(graph: &mut DecisionGraph, observations: &Observations) {
    let mut added = false;
    for item in observations.tracked.iter().filter(|item| item.open) {
        for reference in bracket_ids(&item.text) {
            if reference == item.id
                || reference.starts_with("q-")
                || reference.starts_with("r-")
                || graph.resolve_decision(&reference).is_none()
            {
                continue;
            }
            let exists = graph.edges.iter().any(|edge| {
                edge.kind == EdgeKind::AppliesTo && edge.from == item.id && edge.to == reference
            });
            if exists {
                continue;
            }
            graph.nodes.entry(item.id.clone()).or_insert_with(|| Node {
                id: item.id.clone(),
                kind: NodeKind::Item,
                label: text::question_body(&item.text),
                open: Some(item.open),
                location: None,
                first_seen_at: None,
                in_document: true,
            });
            graph.edges.push(GraphEdge {
                from: item.id.clone(),
                kind: EdgeKind::AppliesTo,
                to: reference,
                source: EdgeSource::Derived,
                clause: None,
            });
            added = true;
        }
    }
    if added {
        graph.edges.sort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_leading_rejection_turns_the_work_down() {
        let q = "Decide whether to add a rate limiter";
        assert!(rejects_work(q, "No, JWT makes it moot."));
        assert!(rejects_work(q, "Moot after the SSO change."));
        assert!(rejects_work(q, "Not needed anymore"));
        assert!(rejects_work(q, "Won't do: the proxy limits it"));
        // A rejection deeper in the answer, a question back, or a bare `No`
        // to a negative question keeps the work.
        assert!(!rejects_work(q, "Yes, ship it; no need to email."));
        assert!(!rejects_work(q, "No?"));
        assert!(!rejects_work("Decide whether to skip the cache", "No."));
        assert!(!rejects_work(q, "Use the standard limiter."));
    }

    #[test]
    fn options_come_from_a_parenthesized_set_or_a_trailing_list() {
        assert_eq!(
            question_options("Choose the cap N (5 or 10)."),
            vec!["10".to_string(), "5".to_string()]
        );
        assert_eq!(
            question_options("Choose the docs site launch date: this Friday or next Monday."),
            vec!["this friday".to_string(), "next monday".to_string()]
        );
        assert_eq!(
            question_options(
                "Choose how to add the demo callback: stop, studio append, or local demo only"
            ),
            vec![
                "stop".to_string(),
                "studio append".to_string(),
                "local demo only".to_string()
            ]
        );
        // Work, not a choice.
        assert!(question_options("Merge PR #10 or PR #11: whichever is green").is_empty());
    }

    #[test]
    fn an_answer_rejects_what_it_negates_or_does_not_choose() {
        let q = "Choose the docs site launch date: this Friday or next Monday.";
        let (chosen, rejected) = rejected_options(q, "Next Monday.");
        assert_eq!(chosen, vec!["next monday".to_string()]);
        assert_eq!(rejected, vec!["this friday".to_string()]);
        let (chosen, rejected) = rejected_options(q, "Not this Friday.");
        assert!(chosen.is_empty());
        assert_eq!(rejected, vec!["this friday".to_string()]);
        let (chosen, rejected) = rejected_options(q, "Ask marketing.");
        assert!(chosen.is_empty() && rejected.is_empty());
    }

    #[test]
    fn restating_needs_most_words_and_every_number() {
        assert!(restates_answer(
            "Use port 8080.",
            "Configure the demo on port 8080"
        ));
        assert!(!restates_answer(
            "Use port 8080.",
            "Configure the demo on port 9090"
        ));
        assert!(!restates_answer(
            "Use the standard http ports.",
            "Configure the demo on port 8080"
        ));
        assert!(restates_answer(
            "Add Alex + Sam",
            "Request review from Alex + Sam on PR #88"
        ));
        assert!(!restates_answer("Yes", "Yes, do it"));
    }

    // A sanitized session where decisions moved under open tracked work.
    const DOC: &str = concat!(
        "---\nagent_doc_session: stale-graph-fixture\n---\n\n",
        "## Main <!-- agent:run id=\"main\" -->\n",
        "- ### Waiting on you <!-- agent-doc:waiting-on-you -->\n",
        "  - 🙋 [#tdport] Choose the demo callback port <!-- agent-doc:waiting-item -->\n",
        "    - ~~Use port 8080.~~\n",
        "    - ~~Use the standard http ports.~~\n",
        "  - 🙋 [#tdlaunch] Choose the docs site launch date: this Friday or next Monday. <!-- agent-doc:waiting-item -->\n",
        "    - ~~Next Monday.~~\n",
        "  - 🙋 [#tdcache] Decide whether to cache the session list <!-- agent-doc:waiting-item -->\n",
        "    - ~~Yes, cache it for five minutes.~~\n",
        "- ### ~~[#ratelimit]: Moot after the SSO change.~~\n",
        "- ### ~~[#livelist]: conflicts with [#tdcache]~~\n",
        "- ### ~~[#tdnotify]: depends on [#tdport]~~\n",
        "<!-- /agent:run -->\n\n",
        "## Todo <!-- agent:todo -->\n",
        "- [ ] [#tdnotify] Email the partners the callback URL\n",
        "<!-- /agent:todo -->\n\n",
        "## Backlog <!-- agent:backlog queue=\"main\" -->\n",
        "- [ ] [#ratelimit] Build the per-user rate limiter.\n",
        "- [ ] [#fridaypost] Schedule the launch post for this Friday (source: [#tdlaunch])\n",
        "- [ ] [#mondaypost] Schedule the launch post for next Monday (source: [#tdlaunch])\n",
        "- [ ] [#portdocs] Write the callback setup docs (source: [#tdport])\n",
        "- [ ] [#portconfig] Configure the demo on port 8080 (source: [#tdport])\n",
        "- [ ] [#livelist] Stream the session list live.\n",
        "- [ ] [#unrelated] Write the onboarding guide.\n",
        "<!-- /agent:backlog -->\n",
    );

    fn stale_of(content: &str) -> Vec<StaleTrackedItem> {
        let observations = crate::extract::observe(content, &[]);
        let ledger = Ledger::build(
            &observations,
            &crate::PriorIndex::default(),
            1_800_000_000,
            None,
        );
        ledger.stale_tracked_items(&observations)
    }

    fn find<'a>(stale: &'a [StaleTrackedItem], item: &str) -> Option<&'a StaleTrackedItem> {
        stale.iter().find(|found| found.item == item)
    }

    #[test]
    fn graph_kinds_flag_moot_superseded_and_contradicting_items() {
        let stale = stale_of(DOC);

        // Work the operator's own answer turns down.
        let rate = find(&stale, "ratelimit").expect("ratelimit moot");
        assert_eq!(rate.kind, StaleKind::MootByDecision, "{rate:?}");
        assert_eq!(rate.link, StaleLink::Identity);
        assert!(rate.action.contains("--done ratelimit"), "{rate:?}");

        // A citing item that assumes the option the answer did not choose.
        let friday = find(&stale, "fridaypost").expect("fridaypost moot");
        assert_eq!(friday.kind, StaleKind::MootByDecision, "{friday:?}");
        assert_eq!(friday.link, StaleLink::AppliesTo);
        assert_eq!(friday.evidence, vec!["option:this friday".to_string()]);
        assert_eq!(friday.answer, "Next Monday.");
        assert!(find(&stale, "mondaypost").is_none(), "{stale:#?}");

        // Built on the superseded port: by citation and by `depends on`.
        let docs = find(&stale, "portdocs").expect("portdocs superseded");
        assert_eq!(docs.kind, StaleKind::BuiltOnSuperseded, "{docs:?}");
        assert_eq!(docs.link, StaleLink::AppliesTo);
        assert_eq!(docs.answer, "Use the standard http ports.");
        assert_eq!(docs.evidence.len(), 1, "{docs:?}");
        assert!(docs.evidence[0].starts_with(&docs.decision_id), "{docs:?}");
        let notify = find(&stale, "tdnotify").expect("tdnotify superseded");
        assert_eq!(notify.kind, StaleKind::BuiltOnSuperseded, "{notify:?}");
        assert_eq!(notify.link, StaleLink::DependsOn);
        assert!(notify.action.contains("--todo-done tdnotify"), "{notify:?}");

        // Restating the superseded answer contradicts the current one.
        let config = find(&stale, "portconfig").expect("portconfig contradicts");
        assert_eq!(config.kind, StaleKind::ContradictsDecision, "{config:?}");
        assert_eq!(config.decision_id, docs.decision_id);
        assert_eq!(config.evidence, docs.evidence);

        // An operator `conflicts with [#id]` link to an answered decision.
        let live = find(&stale, "livelist").expect("livelist contradicts");
        assert_eq!(live.kind, StaleKind::ContradictsDecision, "{live:?}");
        assert_eq!(live.link, StaleLink::ConflictsWith);
        assert_eq!(live.answer, "Yes, cache it for five minutes.");

        assert!(find(&stale, "unrelated").is_none());
        // Flag-only: nothing here closes itself.
        assert!(stale.iter().all(|found| !found.auto_close), "{stale:#?}");
    }

    #[test]
    fn a_rejected_request_about_the_item_is_moot_only_on_its_subject() {
        let side = concat!(
            "<!-- agent:queue -->\n",
            "- ~~[#subagent-operator-request-buildfeature-x1]: No.~~\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:todo -->\n",
            "- [ ] [#buildfeature] Build the allowed-domains feature.\n",
            "- [ ] [#subagent-operator-request-buildfeature-x1] Decide whether to also add browser tests\n",
            "<!-- /agent:todo -->\n",
        );
        let stale = stale_of(side);
        assert!(find(&stale, "buildfeature").is_none(), "{stale:#?}");
        let subject = side.replace(
            "Decide whether to also add browser tests",
            "Decide whether to keep the allowed-domains feature",
        );
        let stale = stale_of(&subject);
        let build = find(&stale, "buildfeature").expect("buildfeature moot");
        assert_eq!(build.kind, StaleKind::MootByDecision, "{build:?}");
        assert_eq!(build.link, StaleLink::AppliesTo);
        assert!(
            build.action.contains("--todo-done buildfeature"),
            "{build:?}"
        );
    }

    #[test]
    fn a_rewrite_citing_the_current_revision_reconciles_the_item() {
        let stale = stale_of(DOC);
        let docs = find(&stale, "portdocs").unwrap();
        assert!(docs.action.contains(&docs.revision_id), "{docs:?}");
        let rewritten = DOC.replace(
            "Write the callback setup docs (source: [#tdport])",
            &format!(
                "Write the callback setup docs for the standard ports ({}) (source: [#tdport])",
                docs.revision_id
            ),
        );
        let stale = stale_of(&rewritten);
        assert!(find(&stale, "portdocs").is_none(), "{stale:#?}");
        assert!(find(&stale, "portconfig").is_some());
    }

    #[test]
    fn an_undelivered_answer_or_live_work_is_not_flagged() {
        // The superseding answer is still live: the agent has not seen it.
        let live_answer = DOC.replace(
            "~~Use the standard http ports.~~",
            "Use the standard http ports.",
        );
        let stale = stale_of(&live_answer);
        for item in ["portdocs", "portconfig", "tdnotify"] {
            assert!(find(&stale, item).is_none(), "{item}: {stale:#?}");
        }
        let live_choice = DOC.replace("~~Next Monday.~~", "Next Monday.");
        let stale = stale_of(&live_choice);
        assert!(find(&stale, "fridaypost").is_none(), "{stale:#?}");
        let live_rejection = DOC.replace(
            "~~[#ratelimit]: Moot after the SSO change.~~",
            "[#ratelimit]: Moot after the SSO change.",
        );
        assert!(find(&stale_of(&live_rejection), "ratelimit").is_none());
        // A live queue row still names the item: that work is in flight.
        let in_flight = DOC.replace(
            "<!-- /agent:run -->",
            "- do [#livelist]\n- do [#fridaypost]\n<!-- /agent:run -->",
        );
        let stale = stale_of(&in_flight);
        assert!(find(&stale, "livelist").is_none(), "{stale:#?}");
        assert!(find(&stale, "fridaypost").is_none(), "{stale:#?}");
    }
}
