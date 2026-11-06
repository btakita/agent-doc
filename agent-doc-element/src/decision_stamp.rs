//! Binary-owned answer-ledger decision stamp (`#decisionidstamp`).
//!
//! One operator question is often rendered as several `agent:todo` copies: the
//! work todo and each request todo raised for it. The answer ledger groups
//! them into one decision with a stable id (`dec-…`). The binary writes that
//! id onto every open copy as the trailing token `⟦decision:<id>⟧`, the same
//! `⟦…⟧` binary-annotation family as `⟦effect:N⟧`, so the document itself
//! carries the decision id and the ledger reads it back instead of deriving
//! it again on every pass.
//!
//! The stamp is not prose. Text comparisons ignore it ([`strip_decision_stamps`]),
//! and it sits at the end of the action's prose, before a trailing canonical
//! `(source: agent:<component> [#<id>])` locator, so the locator stays
//! trailing for every helper that reads it.

/// Opening of a decision stamp token.
pub const DECISION_STAMP_OPEN: &str = "⟦decision:";
const CLOSE: char = '⟧';

fn valid_decision_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Byte ranges `(start, end, id_start, id_end)` of every well-formed stamp.
fn stamp_spans(text: &str) -> Vec<(usize, usize, usize, usize)> {
    let mut spans = Vec::new();
    let mut from = 0;
    while let Some(offset) = text[from..].find(DECISION_STAMP_OPEN) {
        let start = from + offset;
        let id_start = start + DECISION_STAMP_OPEN.len();
        let Some(close) = text[id_start..].find(CLOSE) else {
            break;
        };
        let id_end = id_start + close;
        let end = id_end + CLOSE.len_utf8();
        if valid_decision_id(&text[id_start..id_end]) {
            spans.push((start, end, id_start, id_end));
        }
        from = end;
    }
    spans
}

/// The decision id the first well-formed stamp in `text` names.
pub fn decision_stamp(text: &str) -> Option<&str> {
    stamp_spans(text)
        .first()
        .map(|&(_, _, id_start, id_end)| &text[id_start..id_end])
}

/// `text` without its decision stamps. The space that separated a stamp from
/// the neighbouring text goes with it; every other byte is kept.
pub fn strip_decision_stamps(text: &str) -> String {
    let spans = stamp_spans(text);
    if spans.is_empty() {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    for (start, end, _, _) in spans {
        let mut cut_start = start;
        let mut cut_end = end;
        if text[..start].ends_with(' ') && start > cursor {
            cut_start -= 1;
        } else if text[end..].starts_with(' ') {
            cut_end += 1;
        }
        out.push_str(&text[cursor..cut_start]);
        cursor = cut_end;
    }
    out.push_str(&text[cursor..]);
    out
}

/// Split a trailing canonical `(source: agent:<component> [#<id>])` locator
/// off `action`: `(prose, locator)`.
fn split_source_locator(action: &str) -> (&str, Option<&str>) {
    let marker = " (source: agent:";
    let Some(start) = action.rfind(marker) else {
        return (action, None);
    };
    let tail = &action[start + marker.len()..];
    let well_formed = tail.split_once(" [#").is_some_and(|(component, id_tail)| {
        !component.is_empty()
            && component
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            && id_tail.strip_suffix("])").is_some_and(valid_decision_id)
    });
    if well_formed {
        (&action[..start], Some(action[start + 1..].trim()))
    } else {
        (action, None)
    }
}

/// `action` carrying exactly one stamp for `decision_id`, placed after the
/// prose and before a trailing canonical source locator. Idempotent.
pub fn with_decision_stamp(action: &str, decision_id: &str) -> String {
    let bare = strip_decision_stamps(action);
    let (prose, locator) = split_source_locator(bare.trim_end());
    let prose = prose.trim_end();
    let stamp = format!("{DECISION_STAMP_OPEN}{decision_id}{CLOSE}");
    let mut out = if prose.is_empty() {
        stamp
    } else {
        format!("{prose} {stamp}")
    };
    if let Some(locator) = locator {
        out.push(' ');
        out.push_str(locator);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stamp_goes_before_the_source_locator_and_is_idempotent() {
        let action =
            "Answer the cap question (choices: N = 5, N = 10) (source: agent:review [#tdprovauth])";
        let stamped = with_decision_stamp(action, "dec-0123456789");
        assert_eq!(
            stamped,
            "Answer the cap question (choices: N = 5, N = 10) ⟦decision:dec-0123456789⟧ (source: agent:review [#tdprovauth])"
        );
        assert_eq!(with_decision_stamp(&stamped, "dec-0123456789"), stamped);
        assert_eq!(decision_stamp(&stamped), Some("dec-0123456789"));
        assert_eq!(strip_decision_stamps(&stamped), action);
    }

    #[test]
    fn a_new_id_replaces_the_old_stamp() {
        let stamped = with_decision_stamp("Pick a port ⟦decision:dec-old⟧", "dec-new");
        assert_eq!(stamped, "Pick a port ⟦decision:dec-new⟧");
        assert_eq!(strip_decision_stamps(&stamped), "Pick a port");
    }

    #[test]
    fn malformed_and_other_tokens_are_not_stamps() {
        assert_eq!(decision_stamp("Pick ⟦effect:2⟧"), None);
        assert_eq!(decision_stamp("Pick ⟦decision:has space⟧"), None);
        assert_eq!(strip_decision_stamps("Pick ⟦effect:2⟧"), "Pick ⟦effect:2⟧");
        assert_eq!(
            strip_decision_stamps("⟦decision:dec-a⟧ Pick"),
            "Pick",
            "a leading stamp takes its trailing space"
        );
    }
}
