//! Short response rows (`#responsetreeshape`).
//!
//! A worker hand-back rendered into a response tree used to land as one long
//! `- **Summary:** …` line: a wall of text the operator had to scan for the
//! point. When a prose value is longer than [`PROSE_SPLIT_THRESHOLD`] and has
//! more than one sentence, the row keeps its first sentence as the headline
//! and every later sentence becomes one child bullet beneath it.
//!
//! Only line structure changes, never wording: joining the pieces with one
//! space gives back the input exactly. A sentence never ends inside a code
//! span, a link or other bracketed text, bold text, or an HTML comment, and
//! never after a common abbreviation. Splitting is idempotent: every piece is
//! one sentence, so splitting a piece again yields nothing.

/// Prose longer than this many characters is split into sentences.
pub const PROSE_SPLIT_THRESHOLD: usize = 160;

/// Words that end with a period without ending the sentence.
const ABBREVIATIONS: [&str; 10] = [
    "e.g", "i.e", "etc", "vs", "cf", "approx", "Mr", "Ms", "Dr", "No",
];

/// The sentences of `text` when it is long enough to split
/// ([`PROSE_SPLIT_THRESHOLD`]) and has at least two; `None` otherwise. The
/// pieces are slices of `text` and `pieces.join(" ") == text`.
pub fn split_long_prose(text: &str) -> Option<Vec<&str>> {
    if text.chars().count() <= PROSE_SPLIT_THRESHOLD || text.contains('\n') {
        return None;
    }
    // Review receipts are parsed back from the Summary value as one line.
    let opener = text.trim_start().trim_start_matches('`');
    if opener.starts_with("BLOCKED") || opener.starts_with("REVIEW-CLEAN") {
        return None;
    }
    let ends = sentence_ends(text);
    if ends.is_empty() {
        return None;
    }
    let mut pieces = Vec::with_capacity(ends.len() + 1);
    let mut start = 0;
    for end in ends {
        pieces.push(&text[start..end]);
        // Skip the single separating space.
        start = end + 1;
    }
    pieces.push(&text[start..]);
    Some(pieces)
}

/// Byte offsets just past each sentence end in `text` that is followed by
/// exactly one space and then a sentence opener.
fn sentence_ends(text: &str) -> Vec<usize> {
    let bytes = text.as_bytes();
    let mut ends = Vec::new();
    let mut in_code = false;
    let mut in_bold = false;
    let mut in_comment = false;
    let mut depth = 0usize;
    let mut index = 0;
    while index < bytes.len() {
        let rest = &text[index..];
        if in_comment {
            if rest.starts_with("-->") {
                in_comment = false;
                index += 3;
            } else {
                index += rest.chars().next().map_or(1, char::len_utf8);
            }
            continue;
        }
        if rest.starts_with("<!--") {
            in_comment = true;
            index += 4;
            continue;
        }
        let byte = bytes[index];
        if byte == b'`' {
            in_code = !in_code;
            index += 1;
            continue;
        }
        if in_code {
            index += rest.chars().next().map_or(1, char::len_utf8);
            continue;
        }
        if rest.starts_with("**") {
            in_bold = !in_bold;
            index += 2;
            continue;
        }
        match byte {
            b'[' | b'(' => depth += 1,
            b']' | b')' => depth = depth.saturating_sub(1),
            b'.' | b'?' | b'!' if depth == 0 && !in_bold => {
                let end = index + 1;
                if let Some(after) = text[end..].strip_prefix(' ')
                    && after.chars().next().is_some_and(opens_sentence)
                    && !(byte == b'.' && ends_with_abbreviation(&text[..index]))
                {
                    ends.push(end);
                }
            }
            _ => {}
        }
        index += text[index..].chars().next().map_or(1, char::len_utf8);
    }
    ends
}

fn opens_sentence(next: char) -> bool {
    next.is_uppercase() || next.is_ascii_digit() || "*`[".contains(next)
}

/// Whether the word before a period is an abbreviation or a single letter
/// (`e.g.`, `U.S.`), which does not end a sentence.
fn ends_with_abbreviation(before: &str) -> bool {
    let word = before
        .rsplit(|c: char| c.is_whitespace() || c == '(')
        .next()
        .unwrap_or_default();
    word.chars().count() == 1
        || ABBREVIATIONS
            .iter()
            .any(|abbreviation| word.eq_ignore_ascii_case(abbreviation))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LONG: &str = "The waiting tree now reads a real timestamp. A `### Re: a · b` topic no longer leaks into it. See [the spec. Section 2](https://example.com/a.b) for details, e.g. the repair pass. Tests cover all three cases!";

    #[test]
    fn short_or_single_sentence_prose_is_not_split() {
        assert_eq!(split_long_prose("Done. Verified."), None);
        let one = "word ".repeat(60);
        assert_eq!(split_long_prose(one.trim()), None);
    }

    #[test]
    fn long_prose_splits_on_sentence_boundaries_without_changing_words() {
        let pieces = split_long_prose(LONG).expect("long multi-sentence prose splits");
        assert_eq!(
            pieces,
            vec![
                "The waiting tree now reads a real timestamp.",
                "A `### Re: a · b` topic no longer leaks into it.",
                "See [the spec. Section 2](https://example.com/a.b) for details, e.g. the repair pass.",
                "Tests cover all three cases!",
            ]
        );
        assert_eq!(pieces.join(" "), LONG, "wording is unchanged");
    }

    #[test]
    fn code_spans_links_bold_and_comments_stay_intact() {
        let text = format!(
            "Ran `make check. Then CI` on main and **it passed. Twice** in a row. {} Pushed. Done here <!-- a. B --> now.",
            "x".repeat(120)
        );
        let pieces = split_long_prose(&text).unwrap();
        assert_eq!(pieces.join(" "), text);
        assert!(pieces[0].contains("`make check. Then CI`"), "{pieces:#?}");
        assert!(pieces[0].contains("**it passed. Twice**"), "{pieces:#?}");
        assert!(
            pieces.iter().any(|piece| piece.contains("<!-- a. B -->")),
            "{pieces:#?}"
        );
        for piece in &pieces {
            assert_eq!(piece.matches('`').count() % 2, 0, "{piece}");
            assert_eq!(piece.matches("**").count() % 2, 0, "{piece}");
        }
    }

    #[test]
    fn splitting_is_idempotent() {
        let pieces = split_long_prose(LONG).unwrap();
        for piece in pieces {
            assert_eq!(split_long_prose(piece), None, "{piece}");
        }
    }

    #[test]
    fn review_receipts_stay_one_line() {
        let receipt = format!(
            "BLOCKED independent review. Base=abc head=def cycles=1. {}",
            "More. ".repeat(40)
        );
        assert_eq!(split_long_prose(receipt.trim()), None);
    }
}
