//! External recall for tiers 3-4 and cross-document answers
//! (`#decisiontsiftrecall`).
//!
//! The ledger never runs a search engine itself. An [`AnswerRecall`] source
//! answers "which rows look like this question?" with ranked file/line hits;
//! agent-doc wires `tsift --envelope search` when the project has a tsift
//! index. [`run_recall`] turns those hits into two kinds of annotation:
//!
//! - **in-document neighbours**: a hit on another question row of this same
//!   document feeds the tier-4 [`SemanticNeighbours`] hook through a
//!   [`NeighbourTable`], so [`Ledger::build`] reports it as a
//!   `possibly_answered` candidate;
//! - **cross-document recall**: a hit on an *answered* question (or on its
//!   answer row) in another document becomes a [`RecalledAnswer`].
//!
//! Both are notes, never closes: neither changes a decision's tier, closes a
//! copy, or refuses a closeout. No source (tsift missing, no index, timeout,
//! failure) means no hits, and the ledger keeps its acting tiers 1-2 plus the
//! in-process tier-3 wording check, with no semantic or cross-document notes.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::extract::Observations;
use crate::{Ledger, MatchTier, SemanticNeighbours, id_key, question_keys, text_key};

/// One ranked hit an [`AnswerRecall`] source returned for a query.
#[derive(Debug, Clone, PartialEq)]
pub struct RecallHit {
    /// Path of the file the hit is in, in the same form the caller uses for
    /// `document_path` (agent-doc passes absolute, canonical paths).
    pub path: String,
    /// 1-based line of the hit.
    pub line: usize,
    /// Source score on any scale; [`run_recall`] normalizes it per query so
    /// the best hit is `1.0`.
    pub score: f64,
    /// The hit came from a vector (semantic) backend rather than a lexical one.
    pub semantic: bool,
}

/// A search source for question recall. Implementations must be bounded:
/// on any failure or deadline they return no hits rather than an error.
pub trait AnswerRecall {
    /// Ranked hits for each query, in query order (`result.len()` may be
    /// shorter than `queries.len()`; missing entries mean no hits).
    fn recall(&self, queries: &[String], limit: usize) -> Vec<Vec<RecallHit>>;
}

/// An open, unanswered question this document still asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecallQuery {
    pub decision_index: usize,
    pub decision_id: String,
    pub open_items: Vec<String>,
    pub question: String,
    /// The comparison text sent to the recall source.
    pub text: String,
}

/// A "possibly answered elsewhere" note: an open question here looks like a
/// question the operator answered in another document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecalledAnswer {
    /// Open copies here.
    pub open_items: Vec<String>,
    pub question: String,
    /// The document that holds the answer (display form).
    pub document: String,
    /// The answered decision in that document.
    pub decision_id: String,
    pub answered_question: String,
    /// Latest operator answer there, verbatim.
    pub answer: String,
    pub location: String,
    /// `text` for a lexical hit, `semantic` for a vector hit.
    pub tier: MatchTier,
    /// Normalized recall score in `[0, 1]`.
    pub score: f64,
}

/// Bounds for one recall pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecallOptions {
    /// Open questions sent to the source per pass.
    pub max_queries: usize,
    /// Hits requested per query.
    pub hits_per_query: usize,
    /// Other documents read per pass.
    pub max_documents: usize,
    /// Cross-document notes kept per pass.
    pub max_recalled: usize,
}

impl Default for RecallOptions {
    fn default() -> Self {
        Self {
            max_queries: 3,
            hits_per_query: 8,
            max_documents: 6,
            max_recalled: 5,
        }
    }
}

/// Another document's ledger, built from that document alone so its
/// observation lines are the file's lines.
#[derive(Debug, Clone)]
pub struct OtherDocument {
    /// Display form of the path (for example project-relative).
    pub display: String,
    pub ledger: Ledger,
    pub rows: Observations,
}

/// Tier-4 hook backed by recall hits: pairs of comparison texts with a score.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NeighbourTable {
    pairs: BTreeMap<(String, String), f64>,
}

fn ordered(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}

impl NeighbourTable {
    /// Record that `a` and `b` are neighbours; the best score wins.
    pub fn insert(&mut self, a: &str, b: &str, score: f64) {
        if a.is_empty() || b.is_empty() || a == b {
            return;
        }
        let entry = self.pairs.entry(ordered(a, b)).or_insert(score);
        if score > *entry {
            *entry = score;
        }
    }

    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }
}

impl SemanticNeighbours for NeighbourTable {
    fn similarity(&self, a: &str, b: &str) -> Option<f64> {
        self.pairs.get(&ordered(a, b)).copied()
    }
}

/// What one recall pass produced.
#[derive(Debug, Clone, Default)]
pub struct RecallPass {
    /// Pass to [`Ledger::build`] to get the in-document tier-4 notes.
    pub neighbours: NeighbourTable,
    pub recalled: Vec<RecalledAnswer>,
    /// Queries sent (0 when nothing is open and unanswered).
    pub queries: usize,
}

impl Ledger {
    /// Open, unanswered decisions worth recalling, in decision order.
    pub fn recall_queries(&self, max: usize) -> Vec<RecallQuery> {
        self.decisions
            .iter()
            .enumerate()
            .filter(|(_, decision)| {
                !decision.open_items.is_empty() && decision.revisions.is_empty()
            })
            .filter_map(|(index, decision)| {
                let text = self.profile_text(index);
                (crate::text::content_tokens(text).len() >= 3).then(|| RecallQuery {
                    decision_index: index,
                    decision_id: decision.id.clone(),
                    open_items: decision.open_items.clone(),
                    question: decision.question.clone(),
                    text: text.to_string(),
                })
            })
            .take(max)
            .collect()
    }

    /// The decision a 1-based `line` of the document belongs to: a question
    /// row on that line, or an answer row that addresses one. `rows` must be
    /// the observations of the document text alone (no archives), so its
    /// lines are the file's lines.
    pub fn decision_at_line(&self, rows: &Observations, line: usize) -> Option<usize> {
        let by_question = rows
            .questions
            .iter()
            .filter(|question| question.line == line)
            .find_map(|question| {
                question_keys(question)
                    .iter()
                    .find_map(|key| self.decision_index_for_key(key))
            });
        by_question.or_else(|| {
            rows.answers
                .iter()
                .filter(|answer| answer.line == line)
                .find_map(|answer| {
                    answer
                        .targets
                        .iter()
                        .map(|target| id_key(target))
                        .chain(answer.head_text.as_deref().and_then(text_key))
                        .find_map(|key| self.decision_index_for_key(&key))
                })
        })
    }

    /// True when any key of `other`'s decision resolves here to a decision
    /// that already has an answer: this ledger knows it, no note is needed.
    fn knows_answer_for(&self, keys: &std::collections::BTreeSet<String>) -> bool {
        keys.iter()
            .filter_map(|key| self.decision_for_key(key))
            .any(|decision| !decision.revisions.is_empty())
    }
}

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

/// One recall pass for `ledger` (built from `document_path`; `rows` are the
/// observations of that document's text alone). `load` reads another
/// document by hit path; it returns `None` for anything that is not a
/// readable session document. Hits are notes only.
pub fn run_recall(
    ledger: &Ledger,
    rows: &Observations,
    document_path: &str,
    source: &dyn AnswerRecall,
    options: RecallOptions,
    load: &mut dyn FnMut(&str) -> Option<OtherDocument>,
) -> RecallPass {
    let queries = ledger.recall_queries(options.max_queries);
    let mut pass = RecallPass {
        queries: queries.len(),
        ..RecallPass::default()
    };
    if queries.is_empty() {
        return pass;
    }
    let texts: Vec<String> = queries.iter().map(|query| query.text.clone()).collect();
    let results = source.recall(&texts, options.hits_per_query.max(1));
    let mut others: BTreeMap<String, Option<OtherDocument>> = BTreeMap::new();
    let mut seen: BTreeMap<(usize, String, String), usize> = BTreeMap::new();
    for (query, hits) in queries.iter().zip(results.iter()) {
        for (hit, score) in normalized(hits) {
            if hit.path == document_path {
                if let Some(index) = ledger.decision_at_line(rows, hit.line)
                    && index != query.decision_index
                {
                    pass.neighbours
                        .insert(&query.text, ledger.profile_text(index), score);
                }
                continue;
            }
            if !others.contains_key(&hit.path) {
                let loaded = if others.len() < options.max_documents {
                    load(&hit.path)
                } else {
                    None
                };
                others.insert(hit.path.clone(), loaded);
            }
            let Some(other) = others.get(&hit.path).and_then(Option::as_ref) else {
                continue;
            };
            let Some(index) = other.ledger.decision_at_line(&other.rows, hit.line) else {
                continue;
            };
            let decision = &other.ledger.decisions[index];
            let Some(latest) = decision.latest() else {
                continue;
            };
            if ledger.knows_answer_for(&decision.keys) {
                continue;
            }
            let note = RecalledAnswer {
                open_items: query.open_items.clone(),
                question: query.question.clone(),
                document: other.display.clone(),
                decision_id: decision.id.clone(),
                answered_question: decision.question.clone(),
                answer: latest.answer.clone(),
                location: latest.location.clone(),
                tier: if hit.semantic {
                    MatchTier::Semantic
                } else {
                    MatchTier::Text
                },
                score,
            };
            let key = (query.decision_index, hit.path.clone(), decision.id.clone());
            match seen.get(&key) {
                Some(&at) => {
                    if note.score > pass.recalled[at].score {
                        pass.recalled[at] = note;
                    }
                }
                None => {
                    seen.insert(key, pass.recalled.len());
                    pass.recalled.push(note);
                }
            }
        }
    }
    pass.recalled.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.document.cmp(&b.document))
            .then_with(|| a.decision_id.cmp(&b.decision_id))
    });
    pass.recalled.truncate(options.max_recalled);
    pass
}

/// Cycle-contract guidance for cross-document notes; `None` when empty.
pub fn recall_guidance(recalled: &[RecalledAnswer]) -> Option<String> {
    if recalled.is_empty() {
        return None;
    }
    let mut out = String::from(
        "These open questions may already be answered in another document (#decisiontsiftrecall). These are search matches, not decisions: never close, merge or skip a question on this note alone. If one fits, ask the operator once, quoting the other answer, whether it applies here.",
    );
    for note in recalled {
        out.push_str(&format!(
            "\n- possibly answered elsewhere ({}; {} {:.2}): “{}” — `{}` in `{}` ({})",
            note.open_items
                .iter()
                .map(|item| format!("#{item}"))
                .collect::<Vec<_>>()
                .join(", "),
            match note.tier {
                MatchTier::Semantic => "semantic neighbour",
                _ => "similar wording",
            },
            note.score,
            note.answer,
            note.decision_id,
            note.document,
            note.location
        ));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::observe;
    use crate::{PriorIndex, extract};

    const NOW: u64 = 1_800_000_000;
    const HERE: &str = "/project/tasks/here.md";
    const THERE: &str = "/project/tasks/there.md";

    const LOCAL: &str = concat!(
        "<!-- agent:todo -->\n",
        "- [ ] [#redir] Choose the demo sign-in redirect port for local runs\n",
        "- [ ] [#cb] Choose the local callback port used by the demo login\n",
        "<!-- /agent:todo -->\n",
        "<!-- agent:queue -->\n",
        "- ~~[#cb]: Use port 3000.~~\n",
        "<!-- /agent:queue -->\n",
    );

    const OTHER: &str = concat!(
        "<!-- agent:todo -->\n",
        "- [x] [#portpick] Which port should the demo sign-in redirect use for local runs\n",
        "- [x] [#cbagain] Choose the local callback port used by the demo login\n",
        "<!-- /agent:todo -->\n",
        "<!-- agent:queue -->\n",
        "- ~~[#portpick]: Use port 8443.~~\n",
        "- ~~[#cb]: Use port 3000.~~\n",
        "<!-- /agent:queue -->\n",
    );

    /// A recall source that returns fixed hits for every query.
    struct Fixed(Vec<RecallHit>);

    impl AnswerRecall for Fixed {
        fn recall(&self, queries: &[String], _limit: usize) -> Vec<Vec<RecallHit>> {
            queries.iter().map(|_| self.0.clone()).collect()
        }
    }

    fn hit(path: &str, line: usize, score: f64, semantic: bool) -> RecallHit {
        RecallHit {
            path: path.to_string(),
            line,
            score,
            semantic,
        }
    }

    fn local() -> (Observations, Ledger) {
        let observations = observe(LOCAL, &[]);
        let ledger = Ledger::build(&observations, &PriorIndex::default(), NOW, None);
        (observations, ledger)
    }

    fn load_other(path: &str) -> Option<OtherDocument> {
        (path == THERE).then(|| {
            let rows = extract::observe(OTHER, &[]);
            OtherDocument {
                display: "tasks/there.md".to_string(),
                ledger: Ledger::build(&rows, &PriorIndex::default(), NOW, None),
                rows,
            }
        })
    }

    #[test]
    fn hits_become_neighbour_and_cross_document_notes_only() {
        let (observations, ledger) = local();
        let queries = ledger.recall_queries(3);
        assert_eq!(queries.len(), 1, "{queries:?}");
        assert_eq!(queries[0].open_items, vec!["redir".to_string()]);

        let source = Fixed(vec![
            // This document: the answered callback question (line 3) and the
            // query's own row (line 2), which must be ignored.
            hit(HERE, 3, 4.0, true),
            hit(HERE, 2, 9.0, true),
            // Another document: its answered redirect question, and its copy
            // of a question this document already answered.
            hit(THERE, 2, 4.5, false),
            hit(THERE, 3, 4.5, false),
            // Unreadable documents are skipped.
            hit("/project/notes/random.md", 1, 1.0, false),
        ]);
        let pass = run_recall(
            &ledger,
            &observations,
            HERE,
            &source,
            RecallOptions::default(),
            &mut load_other,
        );
        assert_eq!(pass.queries, 1);
        assert_eq!(pass.neighbours.len(), 1, "{:?}", pass.neighbours);

        // Tier 4 inside the document: a note, never an answer or a close.
        let rebuilt = Ledger::build(
            &observations,
            &PriorIndex::default(),
            NOW,
            Some(&pass.neighbours),
        );
        let note = rebuilt
            .possible
            .iter()
            .find(|candidate| candidate.open_items.contains(&"redir".to_string()))
            .expect("neighbour note");
        assert_eq!(note.tier, MatchTier::Semantic);
        assert_eq!(note.answer, "Use port 3000.");
        assert!(
            rebuilt
                .answered_facts()
                .iter()
                .all(|fact| !fact.open_items.contains(&"redir".to_string()))
        );
        assert!(
            rebuilt
                .duplicate_closures(&observations)
                .iter()
                .all(|closure| closure.todo_id != "redir")
        );

        // Cross-document: only the answer this document does not know.
        assert_eq!(pass.recalled.len(), 1, "{:?}", pass.recalled);
        let recalled = &pass.recalled[0];
        assert_eq!(recalled.open_items, vec!["redir".to_string()]);
        assert_eq!(recalled.document, "tasks/there.md");
        assert_eq!(recalled.answer, "Use port 8443.");
        assert_eq!(recalled.tier, MatchTier::Text);
        assert!((recalled.score - 0.5).abs() < 1e-9, "{recalled:?}");
        let guidance = recall_guidance(&pass.recalled).unwrap();
        assert!(guidance.contains("Use port 8443."), "{guidance}");
        assert!(guidance.contains("never close"), "{guidance}");
    }

    #[test]
    fn an_answer_row_hit_maps_to_its_question() {
        let (observations, ledger) = local();
        let source = Fixed(vec![hit(THERE, 6, 1.0, true)]);
        let pass = run_recall(
            &ledger,
            &observations,
            HERE,
            &source,
            RecallOptions::default(),
            &mut load_other,
        );
        assert_eq!(pass.recalled.len(), 1, "{:?}", pass.recalled);
        assert_eq!(pass.recalled[0].tier, MatchTier::Semantic);
        assert_eq!(pass.recalled[0].answer, "Use port 8443.");
    }

    #[test]
    fn no_source_hits_degrade_to_the_in_process_tiers() {
        let (observations, ledger) = local();
        let pass = run_recall(
            &ledger,
            &observations,
            HERE,
            &Fixed(Vec::new()),
            RecallOptions::default(),
            &mut |_| panic!("no hit, no document read"),
        );
        assert!(pass.neighbours.is_empty() && pass.recalled.is_empty());
        let rebuilt = Ledger::build(
            &observations,
            &PriorIndex::default(),
            NOW,
            Some(&pass.neighbours),
        );
        assert_eq!(rebuilt.possible, ledger.possible);
        assert!(
            rebuilt
                .possible
                .iter()
                .all(|candidate| candidate.tier != MatchTier::Semantic)
        );
        assert_eq!(rebuilt.answered_facts(), ledger.answered_facts());
        assert!(recall_guidance(&pass.recalled).is_none());
    }

    #[test]
    fn nothing_open_sends_no_query_and_reads_are_bounded() {
        let observations = observe(OTHER, &[]);
        let ledger = Ledger::build(&observations, &PriorIndex::default(), NOW, None);
        struct Never;
        impl AnswerRecall for Never {
            fn recall(&self, _: &[String], _: usize) -> Vec<Vec<RecallHit>> {
                panic!("nothing open and unanswered: no query");
            }
        }
        let pass = run_recall(
            &ledger,
            &observations,
            THERE,
            &Never,
            RecallOptions::default(),
            &mut |_| None,
        );
        assert_eq!(pass.queries, 0);

        let (observations, ledger) = local();
        let mut reads = 0;
        let pass = run_recall(
            &ledger,
            &observations,
            HERE,
            &Fixed(vec![hit(THERE, 2, 1.0, false)]),
            RecallOptions {
                max_documents: 0,
                ..RecallOptions::default()
            },
            &mut |path| {
                reads += 1;
                load_other(path)
            },
        );
        assert_eq!(reads, 0);
        assert!(pass.recalled.is_empty());
    }
}
