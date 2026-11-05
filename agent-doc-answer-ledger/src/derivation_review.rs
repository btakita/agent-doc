//! Operator forms that confirm or reject one derivation step
//! (`#firstprinciplesstore`).
//!
//! A derivation (`drv-<hex>`) is one inference step from premises to an
//! answer, stored in the decision graph by `agent-doc decision derive`. The
//! agent authors it as `agent_derived_unreviewed`; only the operator's words
//! in the document change that:
//!
//! - `confirm [#drv-…]` / `reject [#drv-…]` (optionally `: <why>`),
//! - `[#drv-…]: confirm` / `re [#drv-…]: reject — <why>`,
//! - under a waiting item whose own id is `drv-…`: a bare `confirm` /
//!   `reject: <why>` operator row.
//!
//! `confirmed` / `rejected` are accepted for `confirm` / `reject`. A review
//! row is never an answer to a decision, never a question, and never a link.
//! Rejecting a step never rejects its premises.

use serde::Serialize;

/// Derivation ids start with this prefix.
pub const DERIVATION_ID_PREFIX: &str = "drv-";

/// What the operator decided about one derivation step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Confirm,
    Reject,
}

impl Verdict {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Confirm => "confirm",
            Self::Reject => "reject",
        }
    }
}

/// One operator review of a derivation step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DerivationReviewObs {
    /// Lowercase derivation id (`drv-…`).
    pub derivation: String,
    pub verdict: Verdict,
    /// The operator's reason, when given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Human-readable location (`queue row`, `under [#drv-…]`, ...).
    pub location: String,
    /// 1-based line in its source text.
    pub line: usize,
    /// Struck or archived: still on record, outranked by a live row.
    pub consumed: bool,
    /// Read from a done archive.
    pub archived: bool,
}

/// Whether `id` names a derivation.
pub fn is_derivation_id(id: &str) -> bool {
    id.len() > DERIVATION_ID_PREFIX.len()
        && id.to_ascii_lowercase().starts_with(DERIVATION_ID_PREFIX)
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// `confirm` / `confirmed` / `reject` / `rejected` at the start of `text`,
/// followed by a word boundary; returns the verdict and the rest.
fn leading_verdict(text: &str) -> Option<(Verdict, &str)> {
    let lower = text.to_ascii_lowercase();
    for (word, verdict) in [
        ("confirmed", Verdict::Confirm),
        ("confirm", Verdict::Confirm),
        ("rejected", Verdict::Reject),
        ("reject", Verdict::Reject),
    ] {
        if let Some(rest) = lower.strip_prefix(word)
            && rest
                .chars()
                .next()
                .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        {
            return Some((verdict, &text[word.len()..]));
        }
    }
    None
}

/// `[#id]` at the start of `text`; returns the lowercase id and the rest.
fn leading_bracket_id(text: &str) -> Option<(String, &str)> {
    let after = text.strip_prefix("[#")?;
    let end = after.find(']')?;
    let id = &after[..end];
    is_derivation_id(id).then(|| (id.to_ascii_lowercase(), &after[end + 1..]))
}

/// The operator's reason after a verdict: leading `:`, `—`, `-`, `,` or `.`
/// separators are dropped; an empty reason is `None`.
fn note_of(rest: &str) -> Option<String> {
    let note = rest
        .trim()
        .trim_start_matches([':', '—', '–', '-', ',', '.', ';'])
        .trim();
    (!note.is_empty()).then(|| note.to_string())
}

/// Parse one operator row as a derivation review. `own` is the id of the
/// waiting head the row sits under, if any: a bare verdict there reviews that
/// head when it names a derivation. Returns `(derivation id, verdict, note)`.
pub fn parse_review(text: &str, own: Option<&str>) -> Option<(String, Verdict, Option<String>)> {
    let text = text.trim();
    // `confirm [#drv-…]` / `reject [#drv-…]: why`
    if let Some((verdict, rest)) = leading_verdict(text) {
        let rest_trimmed = rest.trim_start();
        if let Some((id, after)) = leading_bracket_id(rest_trimmed) {
            return Some((id, verdict, note_of(after)));
        }
        if let Some(own) = own
            && is_derivation_id(own)
        {
            return Some((own.to_ascii_lowercase(), verdict, note_of(rest)));
        }
        return None;
    }
    // `[#drv-…]: confirm` / `re [#drv-…]: reject — why`
    let addressed = if text.to_ascii_lowercase().starts_with("re [#") {
        &text[3..]
    } else {
        text
    };
    let (id, after) = leading_bracket_id(addressed)?;
    let after = after.trim_start().strip_prefix(':')?.trim_start();
    let (verdict, rest) = leading_verdict(after)?;
    Some((id, verdict, note_of(rest)))
}

/// The review that decides each derivation's status: a live row outranks a
/// consumed one, which outranks an archived one; among equals the later row
/// (by observation order) wins.
pub fn effective_reviews(
    reviews: &[DerivationReviewObs],
) -> std::collections::BTreeMap<String, &DerivationReviewObs> {
    let rank = |review: &DerivationReviewObs| match (review.archived, review.consumed) {
        (true, _) => 0,
        (false, true) => 1,
        (false, false) => 2,
    };
    let mut out: std::collections::BTreeMap<String, &DerivationReviewObs> =
        std::collections::BTreeMap::new();
    for review in reviews {
        match out.get(&review.derivation) {
            Some(current) if rank(current) > rank(review) => {}
            _ => {
                out.insert(review.derivation.clone(), review);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leading_and_addressed_forms() {
        assert_eq!(
            parse_review("confirm [#drv-1a2b3c4d]", None),
            Some(("drv-1a2b3c4d".into(), Verdict::Confirm, None))
        );
        assert_eq!(
            parse_review(
                "Reject [#DRV-1a2b]: the premise is about tokens, not keys",
                None
            ),
            Some((
                "drv-1a2b".into(),
                Verdict::Reject,
                Some("the premise is about tokens, not keys".into())
            ))
        );
        assert_eq!(
            parse_review("[#drv-1a2b]: confirmed", None),
            Some(("drv-1a2b".into(), Verdict::Confirm, None))
        );
        assert_eq!(
            parse_review("re [#drv-1a2b]: reject — wrong scope", None),
            Some((
                "drv-1a2b".into(),
                Verdict::Reject,
                Some("wrong scope".into())
            ))
        );
    }

    #[test]
    fn bare_verdict_reviews_only_a_derivation_head() {
        assert_eq!(
            parse_review("reject: too broad", Some("drv-9f")),
            Some(("drv-9f".into(), Verdict::Reject, Some("too broad".into())))
        );
        assert_eq!(parse_review("confirm", Some("y2fn")), None);
        assert_eq!(parse_review("confirm", None), None);
    }

    #[test]
    fn other_rows_are_not_reviews() {
        for text in [
            "confirm [#y2fn]",
            "[#y2fn]: confirm",
            "confirmation needed for [#drv-1a2b]",
            "[#drv-1a2b]: looks plausible",
            "do [#drv-1a2b]",
            "rejection [#drv-1a2b]",
        ] {
            assert_eq!(parse_review(text, None), None, "{text}");
        }
    }

    #[test]
    fn review_rows_are_observed_and_never_answers() {
        let content = concat!(
            "<!-- agent:run -->\n",
            "- ### Waiting on you <!-- agent-doc:waiting-on-you -->\n",
            "  - 🙋 [#drv-aa11] Confirm the derived answer for the token cap <!-- agent-doc:waiting-item -->\n",
            "    - reject: the fact is about keys\n",
            "  - 🙋 [#tdport] Choose the demo callback port <!-- agent-doc:waiting-item -->\n",
            "    - confirm [#drv-bb22]\n",
            "- [#drv-cc33]: confirmed\n",
            "- ~~confirm [#drv-dd44]~~\n",
            "<!-- /agent:run -->\n",
        );
        let observations = crate::extract::observe(content, &[]);
        let reviews: Vec<(&str, Verdict, bool)> = observations
            .derivation_reviews
            .iter()
            .map(|review| (review.derivation.as_str(), review.verdict, review.consumed))
            .collect();
        assert_eq!(
            reviews,
            vec![
                ("drv-aa11", Verdict::Reject, false),
                ("drv-bb22", Verdict::Confirm, false),
                ("drv-cc33", Verdict::Confirm, false),
                ("drv-dd44", Verdict::Confirm, true),
            ]
        );
        assert_eq!(
            observations.derivation_reviews[0].note.as_deref(),
            Some("the fact is about keys")
        );
        assert!(
            observations.answers.is_empty(),
            "{:?}",
            observations.answers
        );
    }

    #[test]
    fn a_live_review_outranks_consumed_and_archived_ones() {
        let review = |verdict, consumed, archived| DerivationReviewObs {
            derivation: "drv-1".into(),
            verdict,
            note: None,
            location: "queue row".into(),
            line: 1,
            consumed,
            archived,
        };
        let reviews = vec![
            review(Verdict::Confirm, false, false),
            review(Verdict::Reject, false, true),
            review(Verdict::Reject, true, false),
        ];
        assert_eq!(
            effective_reviews(&reviews)["drv-1"].verdict,
            Verdict::Confirm
        );
        let reviews = vec![
            review(Verdict::Confirm, false, false),
            review(Verdict::Reject, false, false),
        ];
        assert_eq!(
            effective_reviews(&reviews)["drv-1"].verdict,
            Verdict::Reject
        );
    }
}
