//! Address-run completion (`#reviewfixonhead`).
//!
//! Addressing a pull-request review is not done until that review's written
//! fix is the code on the exact current head. A later review, a comment that
//! the item is met, or different behavior does not replace the written fix.
//! The hand-back names the head that contains it:
//!
//! ```text
//! FIX-ON-HEAD <review-url> head=<sha>
//! ```
//!
//! A hand-back that omits that line, or that admits the written fix is absent
//! or was superseded, does not complete the address run.

use agent_doc_document::queue_projection::strip_priority_markers;

/// Binding closeout for an `#address` run (`#reviewfixonhead`).
///
/// It overrides a reading that treats a later review, or a different behavior,
/// as addressing a review whose written fix is not on the current head.
pub const ADDRESS_FIX_ON_HEAD_RULE: &str = "Address run (#reviewfixonhead): a review is not addressed until its written fix is the code on the exact current head. The written fix is the code change that review states. Land that change, commit it with the repository's conventions, and publish it only to the existing pull-request head before reporting completion. A later review, a comment that the item is met, or different behavior does not replace that written fix. Do not report the review complete, and do not finish this run, while that fix is absent from the current head. The hand-back names each such review on its own line: `FIX-ON-HEAD <review-url> head=<sha>`. Emit that line only when that head's code is the written fix.";

const ABSENT_FIX_PHRASES: &[&str] = &[
    "written fix is not",
    "written fix is still not",
    "is not the code",
    "not the code",
    "does not do that written fix",
    "does not have that fix",
    "do not have that fix",
    "fix is not on",
    "written fix is absent",
    "were superseded",
    "was superseded",
    "superseded by the review",
    "superseded by a later",
    "not reopened",
];

/// True when `head` references the `#address` preset in `presets`.
pub fn head_is_address_run(head: &str, presets: &[(String, String)]) -> bool {
    crate::queue_response::queue_prompt_preset_expansions_from(
        head,
        presets.iter().map(|(name, body)| (name, body)),
    )
    .iter()
    .any(|(name, _)| name.eq_ignore_ascii_case("#address"))
}

/// True when `item` carries the `#address` preset token.
pub fn item_mentions_address_preset(item: &str) -> bool {
    let text = strip_priority_markers(item);
    let text = crate::queue_continuation::mask_inline_code(&text);
    text.split(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '#' | '-' | '_')))
        .any(|token| token.eq_ignore_ascii_case("#address"))
}

/// `(review url, head sha)` receipts in `response`.
pub fn address_fix_receipts(response: &str) -> Vec<(String, String)> {
    let mut receipts = Vec::new();
    for line in response.lines() {
        if let Some(receipt) = line_fix_receipt(line) {
            receipts.push(receipt);
        }
    }
    receipts
}

/// True when `response` says the written fix is absent, or that a review was
/// superseded instead of applied.
pub fn address_response_admits_fix_absent(response: &str) -> bool {
    let prose = collapsed_prose(response);
    ABSENT_FIX_PHRASES
        .iter()
        .any(|phrase| prose.contains(phrase))
}

/// True when `response` lands the review's written fix on a named head.
///
/// A receipt is necessary and not sufficient: admitting the fix is absent, or
/// that the review was superseded, keeps the run incomplete.
pub fn address_run_lands_written_fix(response: &str) -> bool {
    !address_response_admits_fix_absent(response) && !address_fix_receipts(response).is_empty()
}

fn collapsed_prose(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn line_fix_receipt(line: &str) -> Option<(String, String)> {
    let mut rest = line.trim();
    loop {
        let before = rest;
        rest = rest.trim_start_matches('>').trim_start();
        for opener in ["- ", "* ", "+ "] {
            if let Some(stripped) = rest.strip_prefix(opener) {
                rest = stripped.trim_start();
            }
        }
        rest = rest.trim_start_matches(['*', '_', '`']);
        if rest == before {
            break;
        }
    }
    let rest = rest.strip_prefix("FIX-ON-HEAD")?;
    if !rest.starts_with(|ch: char| ch.is_whitespace()) {
        return None;
    }
    let mut parts = rest.split_whitespace();
    let url = parts.next()?;
    let head = parts.next()?;
    let sha = head.strip_prefix("head=")?;
    let sha = sha.trim_end_matches([',', '.', ';', ')', '`']);
    if !(7..=40).contains(&sha.len()) || !sha.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    Some((url.to_string(), sha.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const REVIEW: &str =
        "https://github.com/sample-org/sample-repo/pull/12#pullrequestreview-5837752992";
    const HEAD: &str = "fa3a3f449986d565f21aeb6e0f792323bc8d1d58";

    fn presets() -> Vec<(String, String)> {
        vec![(
            "#address".to_string(),
            "Address every actionable review item on the exact current head.".to_string(),
        )]
    }

    #[test]
    fn an_address_token_is_an_address_run_and_a_goal_row_is_not() {
        let presets = presets();
        assert!(head_is_address_run(
            "https://github.com/sample-org/sample-repo/pull/12: #address",
            &presets,
        ));
        assert!(!head_is_address_run(
            "https://github.com/sample-org/sample-repo/pull/12: #adversarial-pr",
            &presets,
        ));
        assert!(item_mentions_address_preset("#address [#q-1]"));
        assert!(!item_mentions_address_preset("#adversarial-pr"));
    }

    #[test]
    fn a_superseded_review_without_the_written_fix_does_not_land() {
        let response = format!(
            "Every actionable item on {REVIEW} is already fixed on head 4b1ec500. No new commit.\n\
             Earlier reviews that asked for the shadow apply were superseded by the review above and were not reopened."
        );
        assert!(address_fix_receipts(&response).is_empty());
        assert!(address_response_admits_fix_absent(&response));
        assert!(!address_run_lands_written_fix(&response));
    }

    #[test]
    fn admitting_the_written_fix_is_not_the_code_does_not_land_even_with_a_receipt() {
        let response = format!(
            "FIX-ON-HEAD {REVIEW} head={HEAD}\n\
             The review's written fix, store everything as allowed, is not the code."
        );
        assert!(!address_fix_receipts(&response).is_empty());
        assert!(!address_run_lands_written_fix(&response));
    }

    #[test]
    fn a_receipt_for_the_head_that_contains_the_written_fix_lands() {
        let response = format!(
            "The retry applies under shadow. The stored state, the audit applied_state, and the acknowledgement are allowed.\n\
             FIX-ON-HEAD {REVIEW} head={HEAD}"
        );
        assert_eq!(
            address_fix_receipts(&response),
            vec![(REVIEW.to_string(), HEAD.to_string())]
        );
        assert!(address_run_lands_written_fix(&response));
    }
}
