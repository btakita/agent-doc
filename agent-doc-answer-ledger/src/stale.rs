//! Stale tracked-item audit over the answer ledger (`#decisionstaleaudit`).
//!
//! An open todo, backlog, review or icebox item is *stale* when a decision the
//! ledger already recorded makes it finished, and that decision has been
//! delivered (its answer row is struck, archived, or remembered only by the
//! index), yet the item is still open. Two kinds are derived here from the
//! ledger alone:
//!
//! - [`StaleKind::AnsweredButOpen`]: the item asks a question (its leading
//!   verb is a decision verb, it ends in `?`, or it is a request copy) and its
//!   own key, the item id or its normalized text, has a delivered answer.
//! - [`StaleKind::DoneByEvidence`]: the item names deliverables (PR / issue
//!   numbers, commit SHAs) and a delivered revision of a decision linked to it
//!   (by identity, or a request that `applies_to` it) says every one of them
//!   is merged, landed, pushed, committed, published or released.
//!
//! Only identity-tier todo matches close on their own ([`StaleTrackedItem::auto_close`]):
//! an open `agent:todo` row whose own id is the exact key a delivered operator
//! answer addresses, whose decision has no undelivered revision, that no live
//! queue row or busy waiting tree still works on, and that was never closed
//! for this decision before (a reopened row keeps its `already answered`
//! note, and an operator reopen is authoritative). Everything else is a
//! suggestion the agent applies with `--todo-done` / `--done` /
//! `--review-resolve` citing the decision. Icebox items are only flagged.
//!
//! Kinds that need decision-graph edges (an item made moot by a decision that
//! rejected its premise, an item built on a superseded revision, an item that
//! contradicts the current decision) are not derived here.

use std::collections::BTreeSet;

use serde::Serialize;

use crate::extract::{AnswerObs, Observations, TrackedComponent, TrackedItemObs, is_question_text};
use crate::{
    Decision, DuplicateClosure, EdgeKind, Ledger, Revision, id_key, is_request, text, text_key,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StaleKind {
    AnsweredButOpen,
    DoneByEvidence,
}

impl StaleKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AnsweredButOpen => "answered_but_open",
            Self::DoneByEvidence => "done_by_evidence",
        }
    }
}

/// How the cited decision reaches the item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StaleLink {
    /// The item's own id or normalized text is a key of the decision.
    Identity,
    /// A request raised for the item (`applies_to`) carries the decision.
    AppliesTo,
}

/// One stale tracked item with the decision that makes it stale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StaleTrackedItem {
    pub kind: StaleKind,
    pub component: TrackedComponent,
    pub item: String,
    pub item_text: String,
    pub decision_id: String,
    pub revision_id: String,
    /// The cited revision's operator answer, verbatim.
    pub answer: String,
    pub location: String,
    pub link: StaleLink,
    /// `done_by_evidence`: the deliverables the evidence records as done.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<String>,
    /// The binary closes this item itself (identity-tier todo only).
    pub auto_close: bool,
    /// What the agent does about it.
    pub action: String,
}

/// Words saying a deliverable is finished.
const COMPLETION_WORDS: &[&str] = &[
    "merged",
    "landed",
    "pushed",
    "committed",
    "published",
    "released",
    "shipped",
];

/// Words that negate a nearby completion word (`was not merged`, `isn't
/// merged`, `never landed`). `tokens` drops apostrophes, so `isn't` is `isnt`.
const NEGATIONS: &[&str] = &[
    "not", "never", "no", "nor", "without", "isnt", "wasnt", "hasnt", "havent", "arent", "werent",
    "didnt", "dont", "doesnt", "wont", "cant", "yet", "unless", "until", "before", "once", "after",
    "if", "when",
];

fn is_sha(token: &str) -> bool {
    (7..=40).contains(&token.len())
        && token.chars().all(|c| c.is_ascii_hexdigit())
        && token.chars().any(|c| c.is_ascii_digit())
        && token.chars().any(|c| c.is_ascii_alphabetic())
}

/// Deliverables a text names: `ref:<n>` PR / issue numbers and `sha:<7>`
/// commit prefixes.
pub fn deliverable_anchors(text: &str) -> BTreeSet<String> {
    let mut found: BTreeSet<String> = text::anchors(text)
        .into_iter()
        .filter(|anchor| anchor.starts_with("ref:"))
        .collect();
    for token in text::tokens(&text::question_body(text)) {
        if is_sha(&token) {
            found.insert(format!("sha:{}", &token[..7]));
        }
    }
    found
}

/// Deliverables an answer records as finished: in each sentence that carries
/// an un-negated completion word, every PR / issue number and commit SHA.
pub fn completed_deliverables(answer: &str) -> BTreeSet<String> {
    let mut done = BTreeSet::new();
    let mut sentences = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = answer.chars().collect();
    for (index, ch) in chars.iter().enumerate() {
        let boundary = matches!(ch, ';' | '!' | '\n')
            || (*ch == '.' && chars.get(index + 1).is_none_or(|next| next.is_whitespace()));
        if boundary {
            sentences.push(std::mem::take(&mut current));
        } else {
            current.push(*ch);
        }
    }
    sentences.push(current);
    for sentence in sentences {
        let words = text::tokens(&sentence);
        let completed = words.iter().enumerate().any(|(index, word)| {
            COMPLETION_WORDS.contains(&word.as_str())
                && !words[index.saturating_sub(4)..index]
                    .iter()
                    .any(|before| NEGATIONS.contains(&before.as_str()))
        });
        if completed {
            done.extend(deliverable_anchors(&sentence));
        }
    }
    done
}

/// Whether an item asks the operator something.
fn asks_question(item: &TrackedItemObs) -> bool {
    is_request(&item.id)
        || text::decision_kind(&item.text).is_some()
        || text::question_body(&item.text).contains('?')
}

/// A revision has been delivered: its row is struck, archived, or only the
/// index still remembers it.
fn delivered(revision: &Revision) -> bool {
    revision.consumed
}

fn selector(item: &TrackedItemObs) -> String {
    item.id.clone()
}

fn suggested_action(
    kind: StaleKind,
    item: &TrackedItemObs,
    decision: &Decision,
    revision: &Revision,
    evidence: &[String],
) -> String {
    let id = selector(item);
    let component = item.component.as_str();
    let cite = format!(
        "decision `{}` (“{}”, {})",
        decision.id, revision.answer, revision.location
    );
    let why = match kind {
        StaleKind::AnsweredButOpen => format!(
            "Open agent:{component} item [#{id}] asks a question the operator already answered: {cite}."
        ),
        StaleKind::DoneByEvidence => format!(
            "Open agent:{component} item [#{id}] asks for {} and {cite} already records it done.",
            evidence.join(", ")
        ),
    };
    let what = match item.component {
        TrackedComponent::Todo => format!(
            "If nothing is left for the operator, close it with --todo-done {id} citing `{}`; otherwise rewrite it as the action the answer implies with --todo-edit '{id}=<action>'.",
            decision.id
        ),
        TrackedComponent::Backlog => format!(
            "If the decision finishes it, complete it with --done {id} citing `{}`; otherwise rewrite it as the action the answer implies with --backlog-edit '{id}=<action>'.",
            decision.id
        ),
        TrackedComponent::Review => format!(
            "If the decision settles the gate, resolve it with --review-resolve {id} citing `{}`; otherwise record what is still open with --review-edit '{id}=<text>'.",
            decision.id
        ),
        TrackedComponent::Icebox => format!(
            "Parked work is never closed for you: read the decision and act on it (complete it with --done {id} citing `{}`, move it into active work, or record why it stays parked).",
            decision.id
        ),
    };
    format!("{why} {what} (#decisionstaleaudit)")
}

impl Ledger {
    /// Decisions with an answer that reach `item`, identity first.
    fn linked_decisions(&self, item: &TrackedItemObs) -> Vec<(&Decision, StaleLink)> {
        let mut out: Vec<(&Decision, StaleLink)> = Vec::new();
        let identity = self
            .decision_for_key(&id_key(&item.id))
            .filter(|decision| !decision.revisions.is_empty())
            .or_else(|| {
                text_key(&item.text)
                    .and_then(|key| self.decision_for_key(&key))
                    .filter(|decision| !decision.revisions.is_empty())
            });
        if let Some(decision) = identity {
            out.push((decision, StaleLink::Identity));
        }
        for edge in &self.edges {
            if edge.kind != EdgeKind::AppliesTo || !edge.to.eq_ignore_ascii_case(&item.id) {
                continue;
            }
            if let Some(decision) = self
                .decision_for_key(&id_key(&edge.from))
                .filter(|decision| !decision.revisions.is_empty())
                && !out.iter().any(|(seen, _)| seen.id == decision.id)
            {
                out.push((decision, StaleLink::AppliesTo));
            }
        }
        out
    }

    /// Every open tracked item a delivered decision makes stale, with the
    /// cited decision and the suggested action.
    pub fn stale_tracked_items(&self, observations: &Observations) -> Vec<StaleTrackedItem> {
        let mut out = Vec::new();
        for item in observations.tracked.iter().filter(|item| item.open) {
            // A live queue row still names it: that work is in flight.
            if self.live_queue_ids.contains(&item.id) {
                continue;
            }
            let linked = self.linked_decisions(item);
            if linked.is_empty() {
                continue;
            }
            // An identity-tier close acts; otherwise the most specific
            // finding (evidence of the deliverable) is the suggestion.
            let answered = self.answered_but_open(item, &linked, observations);
            let found = match answered {
                Some(found) if found.auto_close => Some(found),
                answered => self.done_by_evidence(item, &linked).or(answered),
            };
            out.extend(found);
        }
        out
    }

    fn answered_but_open(
        &self,
        item: &TrackedItemObs,
        linked: &[(&Decision, StaleLink)],
        observations: &Observations,
    ) -> Option<StaleTrackedItem> {
        let (decision, _) = linked
            .iter()
            .find(|(_, link)| *link == StaleLink::Identity)?;
        if !asks_question(item) {
            return None;
        }
        let latest = decision.latest()?;
        if !delivered(latest) || is_question_text(&latest.answer) {
            return None;
        }
        let direct: Vec<&AnswerObs> = observations
            .answers
            .iter()
            .filter(|answer| {
                answer
                    .targets
                    .first()
                    .is_some_and(|target| target.eq_ignore_ascii_case(&item.id))
            })
            .collect();
        let reopened = item
            .text
            .contains(&format!("already answered `{}`", decision.id));
        let auto_close = item.component == TrackedComponent::Todo
            && direct.iter().any(|answer| answer.consumed)
            && direct.iter().all(|answer| answer.consumed)
            && decision.revisions.iter().all(delivered)
            && latest.in_document
            && !observations.busy_waiting_ids.contains(&item.id)
            && !reopened;
        Some(StaleTrackedItem {
            kind: StaleKind::AnsweredButOpen,
            component: item.component,
            item: item.id.clone(),
            item_text: text::question_body(&item.text),
            decision_id: decision.id.clone(),
            revision_id: latest.id.clone(),
            answer: latest.answer.clone(),
            location: latest.location.clone(),
            link: StaleLink::Identity,
            evidence: Vec::new(),
            auto_close,
            action: suggested_action(StaleKind::AnsweredButOpen, item, decision, latest, &[]),
        })
    }

    fn done_by_evidence(
        &self,
        item: &TrackedItemObs,
        linked: &[(&Decision, StaleLink)],
    ) -> Option<StaleTrackedItem> {
        let wanted = deliverable_anchors(&item.text);
        if wanted.is_empty() {
            return None;
        }
        for (decision, link) in linked {
            let mut done = BTreeSet::new();
            let mut cited: Option<&Revision> = None;
            for revision in decision
                .revisions
                .iter()
                .filter(|revision| delivered(revision))
            {
                let completed = completed_deliverables(&revision.answer);
                if completed.iter().any(|anchor| wanted.contains(anchor)) {
                    cited = Some(revision);
                }
                done.extend(completed);
            }
            let Some(revision) = cited else { continue };
            if !wanted.is_subset(&done) {
                continue;
            }
            let evidence: Vec<String> = wanted.iter().cloned().collect();
            return Some(StaleTrackedItem {
                kind: StaleKind::DoneByEvidence,
                component: item.component,
                item: item.id.clone(),
                item_text: text::question_body(&item.text),
                decision_id: decision.id.clone(),
                revision_id: revision.id.clone(),
                answer: revision.answer.clone(),
                location: revision.location.clone(),
                link: *link,
                action: suggested_action(
                    StaleKind::DoneByEvidence,
                    item,
                    decision,
                    revision,
                    &evidence,
                ),
                evidence,
                auto_close: false,
            });
        }
        None
    }

    /// The stale todo rows the binary closes itself, as closures
    /// [`crate::apply_duplicate_closures`] applies.
    pub fn stale_auto_closures(&self, observations: &Observations) -> Vec<DuplicateClosure> {
        self.stale_tracked_items(observations)
            .into_iter()
            .filter(|stale| stale.auto_close)
            .map(|stale| DuplicateClosure {
                todo_id: stale.item,
                decision_id: stale.decision_id,
                answer: stale.answer,
                location: stale.location,
                kept_open: None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deliverables_are_pr_numbers_and_commit_prefixes() {
        let found = deliverable_anchors(
            "Fast-forward main to c71880dc60e369edfbe9d070d39fe3cbf94cc953 after docs PR #88 and pull request 91",
        );
        assert!(found.contains("ref:88"), "{found:?}");
        assert!(found.contains("ref:91"), "{found:?}");
        assert!(found.contains("sha:c71880d"), "{found:?}");
        assert!(deliverable_anchors("Implement ADR 0041").is_empty());
    }

    #[test]
    fn completion_needs_an_unnegated_completion_word_in_the_same_sentence() {
        let done = completed_deliverables(
            "Keep the approver as is. PR #88 already merged. PR #90 was not merged; PR #91 is open.",
        );
        assert!(done.contains("ref:88"), "{done:?}");
        assert!(!done.contains("ref:90"), "{done:?}");
        assert!(!done.contains("ref:91"), "{done:?}");
        assert!(completed_deliverables("Merge PR #88 once it is approved.").is_empty());
        assert!(completed_deliverables("Wait until PR #88 is merged.").is_empty());
        let landed = completed_deliverables("v0.1.0 landed at c71880dc60e3 on main");
        assert!(landed.contains("sha:c71880d"), "{landed:?}");
    }
}
