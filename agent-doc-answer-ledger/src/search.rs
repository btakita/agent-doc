//! Decision lookup (`#decisionsearchmenu`): rank a document's answered
//! decisions against a free-form query, and the short lookup menu the cycle
//! contract carries when a turn is about to ask the operator something.
//!
//! The answer ledger already *pushes* the answers of open copies into the
//! contract (`answered_decisions`, `possibly_answered`). That covers questions
//! the document still asks; it does not help an agent that is about to ask a
//! *new* question. [`search_ledger`] is the pull side: given a question text,
//! an id, or a decision stamp, it returns the answered decisions that match,
//! best first, each with the verbatim answer, where and when it was given,
//! and the copies still open.
//!
//! Ranking, best first:
//!
//! 1. **Exact id**: the query names the decision id, one of its revision ids,
//!    or the id of any copy of the question.
//! 2. **Decision stamp**: the query is a `⟦decision:<id>⟧` / `decision:<id>`
//!    stamp (`#decisionidstamp`) of a copy.
//! 3. **Question text**: the query's normalized question text is the text key
//!    of a copy (the identity tier's own comparison).
//! 4. **Applies to**: the query is the id of a work item a request copy of
//!    the decision was raised for (an `applies_to` edge), either way round.
//! 5. **Wording** and **recall**, by score: the share of the query's content
//!    tokens found in the decision's question and answers, and hits from the
//!    optional external recall source ([`crate::recall::AnswerRecall`],
//!    agent-doc wires tsift) mapped back onto answered decisions, in this
//!    document or another one.
//!
//! [`search_registry`] applies the same bands to the project `decisions.md`
//! records (`#decisionsearchnofile`): a record has no question of its own, so
//! its decision text and rationale stand in for one. A record that is no
//! longer `accepted` still answers an exact lookup, but it is labelled, and it
//! ranks after the current records of its band.
//!
//! Everything here is a lookup: nothing closes a copy, merges a decision, or
//! refuses a closeout.

use std::collections::BTreeSet;

use serde::Serialize;

use crate::extract::Observations;
use crate::graph::{
    REGISTRY_LOCATION, RegistryDecision, registry_revision_id, registry_revision_location,
};
use crate::recall::{OtherDocument, RecallHit};
use crate::{Decision, EdgeKind, Ledger, id_key, stamp_key, text, text_key};

/// Minimum share of the query's content tokens a wording match must cover.
pub const WORDING_MIN_COVERAGE: f64 = 0.5;

/// Why a decision matched a query, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchMatch {
    /// The query is the decision id.
    DecisionId,
    /// The query is one of the decision's revision ids.
    RevisionId,
    /// The query is the id of a copy of the question.
    CopyId,
    /// The query is a decision stamp a copy carries.
    Stamp,
    /// Same normalized question text as a copy.
    QuestionText,
    /// The query is the id of an item this decision `applies_to` (a request
    /// raised for that work item, `#answeroutsidewaiting`), or the other way
    /// round.
    AppliesTo,
    /// Shared content tokens (score = share of the query covered).
    Wording,
    /// A lexical hit of the external recall source.
    Recall,
    /// A vector hit of the external recall source.
    RecallSemantic,
}

impl SearchMatch {
    /// The rank band: exact matches order by band, scored matches share the
    /// last band and order by score.
    pub const fn band(self) -> u8 {
        match self {
            Self::DecisionId | Self::RevisionId | Self::CopyId => 0,
            Self::Stamp => 1,
            Self::QuestionText => 2,
            Self::AppliesTo => 3,
            Self::Wording | Self::Recall | Self::RecallSemantic => 4,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DecisionId => "decision id",
            Self::RevisionId => "revision id",
            Self::CopyId => "copy id",
            Self::Stamp => "decision stamp",
            Self::QuestionText => "same question",
            Self::AppliesTo => "applies to the item",
            Self::Wording => "wording",
            Self::Recall => "tsift recall",
            Self::RecallSemantic => "tsift recall (semantic)",
        }
    }
}

/// One answered decision a query found.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SearchHit {
    /// The document that holds the decision (display form).
    pub document: String,
    pub decision_id: String,
    pub question: String,
    /// The latest operator answer, verbatim.
    pub answer: String,
    /// Where the latest answer was given.
    pub location: String,
    /// Unix seconds the binary first saw the latest answer, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_seen_at: Option<u64>,
    /// Earlier answers, oldest first, verbatim.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub earlier_answers: Vec<String>,
    /// Copies of the question that are still open.
    pub open_items: Vec<String>,
    #[serde(rename = "match")]
    pub matched: SearchMatch,
    /// `1.0` for exact matches; coverage or normalized recall score otherwise.
    pub score: f64,
    /// A `decisions.md` record's state (`accepted`, `superseded`, `reversed`,
    /// `deprecated`); absent on a session-document hit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// A `decisions.md` record's rationale.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    /// The `decisions.md` records that supersede this one.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub superseded_by: Vec<String>,
}

impl SearchHit {
    /// False for a registry record that no longer stands: an agent must not
    /// act on its text.
    pub fn is_current(&self) -> bool {
        self.state
            .as_deref()
            .is_none_or(|state| state == "accepted")
    }
}

/// The query, read three ways.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Query {
    /// The query as one bare id (`[#x]`, `#x`, `x`), when it is one token.
    id: Option<String>,
    /// The decision id of a `⟦decision:<id>⟧` / `decision:<id>` stamp.
    stamp: Option<String>,
    /// The query as question text.
    text: String,
}

fn parse_query(raw: &str) -> Query {
    let trimmed = raw.trim();
    let stamp = trimmed
        .trim_start_matches('⟦')
        .trim_end_matches('⟧')
        .strip_prefix("decision:")
        .map(|id| id.trim().to_ascii_lowercase())
        .filter(|id| !id.is_empty() && !id.contains(char::is_whitespace));
    let bare = trimmed
        .trim_start_matches("[#")
        .trim_start_matches('#')
        .trim_end_matches(']')
        .trim();
    let id = (!bare.is_empty() && !bare.contains(char::is_whitespace) && stamp.is_none())
        .then(|| bare.to_ascii_lowercase());
    Query {
        id,
        stamp,
        text: trimmed.to_string(),
    }
}

fn hit_for(
    document: &str,
    decision: &Decision,
    matched: SearchMatch,
    score: f64,
) -> Option<SearchHit> {
    let latest = decision.latest()?;
    Some(SearchHit {
        document: document.to_string(),
        decision_id: decision.id.clone(),
        question: decision.question.clone(),
        answer: latest.answer.clone(),
        location: latest.location.clone(),
        first_seen_at: latest.first_seen_at,
        earlier_answers: decision.revisions[..decision.revisions.len() - 1]
            .iter()
            .map(|revision| revision.answer.clone())
            .collect(),
        open_items: decision.open_items.clone(),
        matched,
        score,
        state: None,
        rationale: None,
        superseded_by: Vec::new(),
    })
}

fn exact_match(decision: &Decision, query: &Query) -> Option<SearchMatch> {
    if let Some(stamp) = &query.stamp {
        return (decision.id.eq_ignore_ascii_case(stamp)
            || decision.keys.contains(&stamp_key(stamp)))
        .then_some(SearchMatch::Stamp);
    }
    if let Some(id) = &query.id {
        if decision.id.eq_ignore_ascii_case(id) {
            return Some(SearchMatch::DecisionId);
        }
        if decision
            .revisions
            .iter()
            .any(|revision| revision.id.eq_ignore_ascii_case(id))
        {
            return Some(SearchMatch::RevisionId);
        }
        if decision.keys.contains(&id_key(id)) {
            return Some(SearchMatch::CopyId);
        }
        if decision.keys.contains(&stamp_key(id)) {
            return Some(SearchMatch::Stamp);
        }
    }
    let key = text_key(&query.text)?;
    decision
        .keys
        .contains(&key)
        .then_some(SearchMatch::QuestionText)
}

/// Share of the query's content tokens found in `tokens`; `None` below
/// [`WORDING_MIN_COVERAGE`] or when the query names a number `tokens` lacks.
fn coverage(tokens: &BTreeSet<String>, query_tokens: &BTreeSet<String>) -> Option<f64> {
    if query_tokens.is_empty() {
        return None;
    }
    let shared = query_tokens.intersection(tokens).count();
    let only_query_numbers = query_tokens
        .difference(tokens)
        .any(|token| token.chars().all(|c| c.is_ascii_digit() || c == '.'));
    if shared == 0 || only_query_numbers {
        return None;
    }
    let score = shared as f64 / query_tokens.len() as f64;
    (score >= WORDING_MIN_COVERAGE).then_some(score)
}

/// [`coverage`] over the decision's question (every copy's comparison text)
/// and answers.
fn wording_score(ledger: &Ledger, index: usize, query_tokens: &BTreeSet<String>) -> Option<f64> {
    let decision = &ledger.decisions[index];
    let mut tokens = text::content_tokens(&decision.question);
    tokens.extend(text::content_tokens(ledger.profile_text(index)));
    for revision in &decision.revisions {
        tokens.extend(text::content_tokens(&revision.answer));
    }
    coverage(&tokens, query_tokens)
}

/// The answered decisions of `ledger` that match `query`, best first. Only
/// in-process comparisons; see [`add_recall_hits`] for the recall source.
pub fn search_ledger(ledger: &Ledger, query: &str, document: &str) -> Vec<SearchHit> {
    let parsed = parse_query(query);
    let query_tokens = text::content_tokens(&parsed.text);
    let mut hits = Vec::new();
    for (index, decision) in ledger.decisions.iter().enumerate() {
        if decision.revisions.is_empty() {
            continue;
        }
        let found = match exact_match(decision, &parsed) {
            Some(matched) => hit_for(document, decision, matched, 1.0),
            None if parsed.stamp.is_some() => None,
            None => wording_score(ledger, index, &query_tokens)
                .and_then(|score| hit_for(document, decision, SearchMatch::Wording, score)),
        };
        hits.extend(found);
    }
    if let Some(id) = &parsed.id {
        for edge in ledger
            .edges
            .iter()
            .filter(|edge| edge.kind == EdgeKind::AppliesTo)
        {
            let other = if edge.to.eq_ignore_ascii_case(id) {
                &edge.from
            } else if edge.from.eq_ignore_ascii_case(id) {
                &edge.to
            } else {
                continue;
            };
            if let Some(decision) = ledger.decision_for_key(&id_key(other)) {
                hits.extend(hit_for(document, decision, SearchMatch::AppliesTo, 1.0));
            }
        }
    }
    rank(&mut hits);
    hits
}

/// How a `decisions.md` record matches a query exactly, if it does.
fn registry_match(record: &RegistryDecision, query: &Query) -> Option<SearchMatch> {
    if let Some(stamp) = &query.stamp {
        return record
            .id
            .eq_ignore_ascii_case(stamp)
            .then_some(SearchMatch::Stamp);
    }
    if let Some(id) = &query.id {
        if record.id.eq_ignore_ascii_case(id) {
            return Some(SearchMatch::DecisionId);
        }
        if registry_revision_id(record).eq_ignore_ascii_case(id) {
            return Some(SearchMatch::RevisionId);
        }
    }
    let key = text_key(&query.text)?;
    (text_key(&record.decision).as_ref() == Some(&key)).then_some(SearchMatch::QuestionText)
}

/// The project `decisions.md` records that match `query`, best first
/// (`#decisionsearchnofile`).
pub fn search_registry(records: &[RegistryDecision], query: &str) -> Vec<SearchHit> {
    let parsed = parse_query(query);
    let query_tokens = text::content_tokens(&parsed.text);
    let mut hits = Vec::new();
    for record in records {
        let found = match registry_match(record, &parsed) {
            Some(matched) => Some((matched, 1.0)),
            None if parsed.stamp.is_some() => None,
            None => {
                let mut tokens = text::content_tokens(&record.decision);
                tokens.extend(text::content_tokens(&record.rationale));
                coverage(&tokens, &query_tokens).map(|score| (SearchMatch::Wording, score))
            }
        };
        let Some((matched, score)) = found else {
            continue;
        };
        hits.push(SearchHit {
            document: REGISTRY_LOCATION.to_string(),
            decision_id: record.id.clone(),
            question: format!("project decision ({})", record.state),
            answer: record.decision.clone(),
            location: registry_revision_location(record),
            first_seen_at: None,
            earlier_answers: Vec::new(),
            open_items: Vec::new(),
            matched,
            score,
            state: Some(record.state.clone()),
            rationale: Some(record.rationale.clone()),
            superseded_by: records
                .iter()
                .filter(|other| other.supersedes.contains(&record.id))
                .map(|other| other.id.clone())
                .collect(),
        });
    }
    rank(&mut hits);
    hits
}

/// Normalize recall scores per query so the best hit is `1.0`.
fn normalized(hits: &[RecallHit]) -> Vec<(&RecallHit, f64)> {
    let top = hits
        .iter()
        .map(|hit| hit.score)
        .filter(|score| score.is_finite() && *score > 0.0)
        .fold(0.0_f64, f64::max);
    hits.iter()
        .map(|hit| {
            let score = if top > 0.0 && hit.score.is_finite() {
                (hit.score / top).clamp(0.0, 1.0)
            } else {
                0.0
            };
            (hit, score)
        })
        .collect()
}

/// A document [`add_recall_hits`] already has a ledger for.
pub struct SearchDocument<'a> {
    pub ledger: &'a Ledger,
    /// Observations of the document text alone (no archives), so its lines
    /// are the file's lines.
    pub rows: &'a Observations,
    /// Path in the form recall hits use (absolute, canonical).
    pub path: &'a str,
    pub display: &'a str,
    /// False when the recall index is older than the document: its line
    /// numbers here may be stale, so hits on this document are skipped.
    pub hits_current: bool,
}

/// Map recall hits onto answered decisions (one of `documents`, or another
/// readable session document's through `load`) and merge them into `hits`.
pub fn add_recall_hits(
    hits: &mut Vec<SearchHit>,
    documents: &[SearchDocument<'_>],
    recall: &[RecallHit],
    max_documents: usize,
    load: &mut dyn FnMut(&str) -> Option<OtherDocument>,
) {
    let mut others: std::collections::BTreeMap<String, Option<OtherDocument>> =
        std::collections::BTreeMap::new();
    for (hit, score) in normalized(recall) {
        let matched = if hit.semantic {
            SearchMatch::RecallSemantic
        } else {
            SearchMatch::Recall
        };
        if let Some(here) = documents.iter().find(|document| document.path == hit.path) {
            if !here.hits_current {
                continue;
            }
            if let Some(index) = here.ledger.decision_at_line(here.rows, hit.line) {
                hits.extend(hit_for(
                    here.display,
                    &here.ledger.decisions[index],
                    matched,
                    score,
                ));
            }
            continue;
        }
        if !others.contains_key(&hit.path) {
            let loaded = if others.len() < max_documents {
                load(&hit.path)
            } else {
                None
            };
            others.insert(hit.path.clone(), loaded);
        }
        let Some(other) = others.get(&hit.path).and_then(Option::as_ref) else {
            continue;
        };
        if let Some(index) = other.ledger.decision_at_line(&other.rows, hit.line) {
            hits.extend(hit_for(
                &other.display,
                &other.ledger.decisions[index],
                matched,
                score,
            ));
        }
    }
    rank(hits);
}

/// Best first, one hit per (document, decision): the better band wins, then
/// a record that still stands over one that does not, then the higher score,
/// then document order and decision id.
pub fn rank(hits: &mut Vec<SearchHit>) {
    hits.sort_by(|a, b| {
        a.matched
            .band()
            .cmp(&b.matched.band())
            .then_with(|| b.is_current().cmp(&a.is_current()))
            .then_with(|| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| a.matched.cmp(&b.matched))
            .then_with(|| a.document.cmp(&b.document))
            .then_with(|| a.decision_id.cmp(&b.decision_id))
    });
    let mut seen = BTreeSet::new();
    hits.retain(|hit| seen.insert((hit.document.clone(), hit.decision_id.clone())));
}

/// Why this turn is about to ask the operator something.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "cue", content = "detail")]
pub enum QuestionCue {
    /// The turn's prompt names an open item that asks the operator something
    /// (an open question copy, or a tracked item worded as a decision).
    PromptNamesQuestion(String),
    /// The turn's prompt itself asks for a decision (`choose …`, `decide …`,
    /// `give me choices`, `ask me …`).
    PromptAsksDecision,
    /// Open copies of questions the ledger already has an answer or a
    /// candidate answer for: the turn is about to act on, or re-ask, them.
    AnsweredCopiesOpen,
}

/// Whether a row's text asks the operator for something: a decision verb
/// leads it, or it ends with `?`.
fn asks(text: &str) -> bool {
    text::decision_kind(text).is_some() || crate::extract::is_question_text(text)
}

/// Ids (lowercase) of open items that ask the operator something: open
/// waiting heads and request packets, and open todo, backlog, review and
/// icebox items whose text asks for a decision or ends with `?` (a work item
/// such as `Implement …` is not a question).
pub fn question_item_ids(observations: &Observations) -> BTreeSet<String> {
    let mut ids: BTreeSet<String> = observations
        .questions
        .iter()
        .filter(|question| {
            question.open
                && (question.source != crate::extract::QuestionSource::Todo || asks(&question.text))
        })
        .filter_map(|question| question.id.as_deref())
        .map(str::to_ascii_lowercase)
        .collect();
    ids.extend(
        observations
            .tracked
            .iter()
            .filter(|item| item.open && asks(&item.text))
            .map(|item| item.id.to_ascii_lowercase()),
    );
    ids
}

/// The ids a prompt names (`[#x]` and bare `#x` tokens), lowercase.
fn prompt_ids(prompt: &str) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    let mut rest = prompt;
    while let Some(at) = rest.find('#') {
        let after = &rest[at + 1..];
        let id: String = after
            .chars()
            .take_while(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
            .collect();
        let id = id.trim_end_matches('.');
        if !id.is_empty() {
            ids.insert(id.to_ascii_lowercase());
        }
        rest = after;
    }
    ids
}

/// Whether an operator prompt asks for a decision the agent would put back
/// to the operator: it leads with choose/pick/select, or it says `decide`,
/// `choices` or `ask me`. A plain `which …?` is the operator asking the
/// agent, and `I authorize …` / `approve …` grant rather than ask; neither
/// is a cue.
fn prompt_asks_decision(prompt: &str) -> bool {
    if text::decision_kind(prompt) == Some("choose") {
        return true;
    }
    let words = text::tokens(&text::question_body(prompt));
    words
        .iter()
        .any(|word| matches!(word.as_str(), "decide" | "choices"))
        || words
            .windows(2)
            .any(|pair| pair[0] == "ask" && pair[1] == "me")
}

/// Inputs for [`question_cue`].
pub struct CueInputs<'a> {
    /// This turn's operator prompts (selected queue heads, prompt changes,
    /// chat prompts), verbatim.
    pub prompts: &'a [&'a str],
    /// [`question_item_ids`] of the document.
    pub question_items: &'a BTreeSet<String>,
    /// Answered decisions the document's ledger holds.
    pub answered_decisions: usize,
    /// The contract already pushes an answer or a candidate for an open copy
    /// (`answered_decisions`, `possibly_answered`, recall notes).
    pub answers_pushed: bool,
}

/// Whether this turn is about to ask the operator something, and why;
/// `None` for an ordinary work turn. With nothing answered to look up
/// (no answered decision here, nothing pushed), there is no cue.
pub fn question_cue(inputs: &CueInputs<'_>) -> Option<QuestionCue> {
    if inputs.answered_decisions == 0 && !inputs.answers_pushed {
        return None;
    }
    for prompt in inputs.prompts {
        if let Some(id) = prompt_ids(prompt)
            .into_iter()
            .find(|id| inputs.question_items.contains(id))
        {
            return Some(QuestionCue::PromptNamesQuestion(id));
        }
    }
    if inputs
        .prompts
        .iter()
        .any(|prompt| prompt_asks_decision(prompt))
    {
        return Some(QuestionCue::PromptAsksDecision);
    }
    inputs
        .answers_pushed
        .then_some(QuestionCue::AnsweredCopiesOpen)
}

/// The short decision-lookup menu for `file` (display form).
pub fn lookup_menu(file: &str) -> String {
    format!(
        "Before asking the operator anything (#decisionsearchmenu), look it up: `agent-doc decision search {file} <question, id or stamp>` ranks the answered decisions; `agent-doc decision answered {file}` lists them; `agent-doc decision why {file} <id>` shows why one holds. Drop the file to search the whole project. Ask only when nothing answers it, and cite the decision id you checked."
    )
}

#[cfg(test)]
mod tests;
