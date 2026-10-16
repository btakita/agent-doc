//! Committed-text integrity guard (GH #274).
//!
//! ## Invariant
//!
//! Recovery never commits a candidate that **adds** a copy of already-committed
//! text: neither a second copy of a committed response block, nor a line that
//! differs from a committed line only by encoder replacement characters.
//!
//! A "copy" of a response block is judged modulo binary-owned decoration
//! (`#dupemoduloecho`, [`crate::response_replay::response_block_signature`]):
//! `#closeoutduplicatedcommit` committed a second copy that differed from the
//! first only by a binary done-id `> **Queue prompt:**` echo on one side and
//! repair's `❯ ` prompt prefixes on the other, and a byte-exact comparison let
//! it through.
//!
//! GH #274: an IDE whose JVM ran under an ASCII locale marshalled document text
//! across JNA with US-ASCII, so every non-ASCII character of a committed response
//! (`—`, `·`, `→`, emoji) became a literal `?`. The lossy copy landed above the
//! correct one, and a preflight repair adopted the document and committed both.
//! Agent-authored text is never produced by an encoder, so a line that matches a
//! committed line only after its non-ASCII characters are replaced is corruption,
//! not authorship, and the recovery must fail closed rather than commit it.
//!
//! The comparison is relative to committed `HEAD`: corruption that is already
//! committed, and duplicates `HEAD` already carries, are not "added" by the
//! candidate and do not trip the guard.

use std::collections::{HashMap, HashSet};

use crate::response_replay::response_blocks;

/// Minimum ASCII alphanumerics a lossy line must keep before it can be called a
/// rendering of a committed line. Short lines like `→` vs an operator's `?` must
/// never read as corruption.
const MIN_ASCII_ALNUM_FOR_LOSSY_MATCH: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommittedTextViolationKind {
    /// The candidate carries more exact copies of a committed response block
    /// than committed `HEAD` does.
    DuplicateCommittedResponse,
    /// The candidate carries a line that equals a committed line once every
    /// non-ASCII character (or UTF-8 byte) is replaced by `?` or U+FFFD.
    LossyCommittedTextCopy,
}

impl CommittedTextViolationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DuplicateCommittedResponse => "duplicate_committed_response",
            Self::LossyCommittedTextCopy => "lossy_committed_text_copy",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommittedTextViolation {
    pub kind: CommittedTextViolationKind,
    /// The committed line the candidate copies (the response heading for a
    /// duplicate block).
    pub committed_line: String,
    /// The candidate's offending line.
    pub candidate_line: String,
}

/// The four renderings of `line` that a lossy encode/decode produces:
/// each non-ASCII scalar → `?` (a Java `String` encoded to US-ASCII — JNA under
/// an ASCII `native.encoding`), each scalar → U+FFFD, and each non-ASCII UTF-8
/// byte → `?` or U+FFFD (bytes decoded as ASCII, optionally re-encoded).
/// Returns an empty list for an all-ASCII line: there is nothing to lose.
pub fn lossy_renderings(line: &str) -> Vec<String> {
    if line.is_ascii() {
        return Vec::new();
    }
    let per_char = |replacement: char| -> String {
        line.chars()
            .map(|ch| if ch.is_ascii() { ch } else { replacement })
            .collect()
    };
    let per_byte = |replacement: char| -> String {
        let mut out = String::with_capacity(line.len());
        for ch in line.chars() {
            if ch.is_ascii() {
                out.push(ch);
            } else {
                for _ in 0..ch.len_utf8() {
                    out.push(replacement);
                }
            }
        }
        out
    };
    let mut renderings = vec![
        per_char('?'),
        per_char('\u{FFFD}'),
        per_byte('?'),
        per_byte('\u{FFFD}'),
    ];
    renderings.sort();
    renderings.dedup();
    renderings
}

/// Whether `candidate` is a lossy rendering of `committed` (and not `committed`
/// itself).
pub fn is_lossy_rendering_of(candidate: &str, committed: &str) -> bool {
    candidate != committed
        && ascii_alnum_count(candidate) >= MIN_ASCII_ALNUM_FOR_LOSSY_MATCH
        && lossy_renderings(committed)
            .iter()
            .any(|rendering| rendering == candidate)
}

fn ascii_alnum_count(line: &str) -> usize {
    line.bytes().filter(u8::is_ascii_alphanumeric).count()
}

/// Return the first way `candidate` adds a copy of committed text, or `None`
/// when every committed response block and line appears no more often than
/// `committed` itself carries it, and nothing is a lossy rendering of it.
pub fn committed_text_violation(
    candidate: &str,
    committed: &str,
) -> Option<CommittedTextViolation> {
    lossy_committed_line(candidate, committed)
        .or_else(|| duplicate_committed_response_block(candidate, committed))
}

/// Return the first line pair the candidate **adds** in which one line is a
/// lossy rendering of the other, or `None`.
///
/// This is the projection-side half of the invariant: GH #274's re-register
/// projection started from an editor baseline that already held the lossy
/// copy, then replayed the correct response next to it. Neither line alone is
/// new relative to committed text, but the candidate is the first text to
/// carry *both* the original and its encoder-replaced rendering. A pair the
/// base already carries is not added and never refuses.
pub fn added_lossy_copy_pair(candidate: &str, base: &str) -> Option<CommittedTextViolation> {
    let candidate_lines: HashSet<&str> = candidate.lines().map(str::trim).collect();
    let base_lines: HashSet<&str> = base.lines().map(str::trim).collect();
    let mut originals: Vec<&str> = candidate_lines
        .iter()
        .copied()
        .filter(|line| !line.is_ascii())
        .collect();
    originals.sort_unstable();
    originals.into_iter().find_map(|original| {
        lossy_renderings(original)
            .into_iter()
            .find(|rendering| {
                ascii_alnum_count(rendering) >= MIN_ASCII_ALNUM_FOR_LOSSY_MATCH
                    && candidate_lines.contains(rendering.as_str())
                    && !(base_lines.contains(original) && base_lines.contains(rendering.as_str()))
            })
            .map(|rendering| CommittedTextViolation {
                kind: CommittedTextViolationKind::LossyCommittedTextCopy,
                committed_line: original.to_string(),
                candidate_line: rendering,
            })
    })
}

fn lossy_committed_line(candidate: &str, committed: &str) -> Option<CommittedTextViolation> {
    let committed_lines: HashSet<&str> = committed.lines().map(str::trim).collect();
    let mut by_rendering: HashMap<String, &str> = HashMap::new();
    for line in &committed_lines {
        for rendering in lossy_renderings(line) {
            by_rendering.entry(rendering).or_insert(line);
        }
    }
    if by_rendering.is_empty() {
        return None;
    }
    candidate
        .lines()
        .map(str::trim)
        .filter(|line| !committed_lines.contains(line))
        .filter(|line| ascii_alnum_count(line) >= MIN_ASCII_ALNUM_FOR_LOSSY_MATCH)
        .find_map(|line| {
            by_rendering
                .get(line)
                .map(|committed_line| CommittedTextViolation {
                    kind: CommittedTextViolationKind::LossyCommittedTextCopy,
                    committed_line: (*committed_line).to_string(),
                    candidate_line: line.to_string(),
                })
        })
}

/// Return the first response block the candidate carries more copies of than
/// `base` does, or `None` (`#closeoutduplicatedcommit`).
///
/// Blocks are compared by [`crate::response_replay::response_block_signature`],
/// so a copy that differs only by binary-owned decoration — a done-id
/// `> **Queue prompt:**` echo, `❯ ` prompt prefixes, boundary markers, the
/// `(HEAD)` suffix, blank lines — is still a copy. Materialization sites call
/// this with their own pre-image as `base`: a recovery target that re-appends a
/// response the document already holds is refused before it reaches an editor.
pub fn added_response_copy(candidate: &str, base: &str) -> Option<CommittedTextViolation> {
    duplicate_committed_response_block(candidate, base)
}

fn duplicate_committed_response_block(
    candidate: &str,
    committed: &str,
) -> Option<CommittedTextViolation> {
    let mut committed_counts: HashMap<String, usize> = HashMap::new();
    for block in response_blocks(committed) {
        *committed_counts.entry(block).or_default() += 1;
    }
    let mut candidate_counts: HashMap<String, usize> = HashMap::new();
    for block in response_blocks(candidate) {
        if committed_counts.contains_key(&block) {
            *candidate_counts.entry(block).or_default() += 1;
        }
    }
    let mut surplus: Vec<&String> = candidate_counts
        .iter()
        .filter(|(block, count)| **count > committed_counts[*block])
        .map(|(block, _)| block)
        .collect();
    surplus.sort();
    surplus.first().map(|block| {
        let heading = block.lines().next().unwrap_or_default().to_string();
        CommittedTextViolation {
            kind: CommittedTextViolationKind::DuplicateCommittedResponse,
            committed_line: heading.clone(),
            candidate_line: heading,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// The JNA defect's exact transform: a Java `String` encoded with US-ASCII
    /// and `REPLACE` — one `?` per Unicode scalar (a surrogate pair is one `?`).
    fn jna_ascii_encode(text: &str) -> String {
        text.chars()
            .map(|ch| if ch.is_ascii() { ch } else { '?' })
            .collect()
    }

    const COMMITTED_RESPONSE: &str = concat!(
        "### Re: upgrade to 0.35.508 and viewpane issue dispatched \u{2014} opus-5.5 \u{b7} 2026-10-15T11:20-04:00\n",
        "- **Upgraded 0.35.505 \u{2192} 0.35.508** (`agent-doc upgrade`, exit 0).\n",
        "  - `#upgrade`: checks gh 261\u{2013}267 against 0.35.508.\n",
    );

    fn committed_doc() -> String {
        format!(
            "<!-- agent:exchange patch=append -->\n\u{276f} upgrade please\n{COMMITTED_RESPONSE}<!-- /agent:exchange -->\n"
        )
    }

    #[test]
    fn gh274_lossy_copy_above_committed_response_is_refused() {
        let committed = committed_doc();
        let lossy = jna_ascii_encode(COMMITTED_RESPONSE);
        let candidate =
            committed.replace(COMMITTED_RESPONSE, &format!("{lossy}{COMMITTED_RESPONSE}"));
        let violation = committed_text_violation(&candidate, &committed).expect("refused");
        assert_eq!(
            violation.kind,
            CommittedTextViolationKind::LossyCommittedTextCopy
        );
        assert!(violation.candidate_line.contains("dispatched ? opus-5.5 ?"));
        assert!(
            violation
                .committed_line
                .contains("dispatched \u{2014} opus-5.5")
        );
    }

    #[test]
    fn lossy_replacement_of_committed_text_is_refused() {
        let committed = committed_doc();
        let candidate = jna_ascii_encode(&committed);
        assert_eq!(
            committed_text_violation(&candidate, &committed).map(|v| v.kind),
            Some(CommittedTextViolationKind::LossyCommittedTextCopy)
        );
    }

    #[test]
    fn utf8_bytes_decoded_as_ascii_are_refused() {
        let committed = committed_doc();
        let per_byte: String = COMMITTED_RESPONSE
            .chars()
            .flat_map(|ch| {
                if ch.is_ascii() {
                    vec![ch]
                } else {
                    vec!['\u{FFFD}'; ch.len_utf8()]
                }
            })
            .collect();
        let candidate = format!("{committed}{per_byte}");
        assert!(committed_text_violation(&candidate, &committed).is_some());
    }

    #[test]
    fn exact_second_copy_of_committed_response_is_refused() {
        let committed = committed_doc();
        let candidate = committed.replace(
            COMMITTED_RESPONSE,
            &format!("{COMMITTED_RESPONSE}{COMMITTED_RESPONSE}"),
        );
        let violation = committed_text_violation(&candidate, &committed).expect("refused");
        assert_eq!(
            violation.kind,
            CommittedTextViolationKind::DuplicateCommittedResponse
        );
        assert!(violation.committed_line.starts_with("### Re: upgrade"));
    }

    /// `#closeoutduplicatedcommit`: HEAD carries the response once; the
    /// candidate carries it twice, one copy under a binary done-id echo and the
    /// other with `❯`-prefixed body lines. Neither copy is byte-equal to HEAD's,
    /// and the guard still refuses.
    #[test]
    fn copy_modulo_binary_echo_and_prompt_prefixes_is_refused() {
        let committed = committed_doc();
        let echoed = COMMITTED_RESPONSE.replacen(
            "\n",
            "\n\n> **Queue prompt:**\n>\n> do [#upgrade]\n>\n> do [#viewpane]\n\n",
            1,
        );
        let prefixed = COMMITTED_RESPONSE.replace("- **Upgraded", "\u{276f} - **Upgraded");
        let candidate = committed.replace(COMMITTED_RESPONSE, &format!("{echoed}{prefixed}"));
        let violation = committed_text_violation(&candidate, &committed).expect("refused");
        assert_eq!(
            violation.kind,
            CommittedTextViolationKind::DuplicateCommittedResponse
        );
        assert!(violation.committed_line.starts_with("### Re: upgrade"));
        // A single decorated copy replacing the committed one adds nothing.
        let single = committed.replace(COMMITTED_RESPONSE, &echoed);
        assert_eq!(committed_text_violation(&single, &committed), None);
        assert_eq!(added_response_copy(&single, &committed), None);
        assert!(added_response_copy(&candidate, &single).is_some());
    }

    #[test]
    fn new_response_and_operator_edits_pass() {
        let committed = committed_doc();
        let candidate = committed.replace(
            "<!-- /agent:exchange -->",
            "\u{276f} why?\n?\nok ? fine\n### Re: follow-up \u{2014} opus-5.5 \u{b7} 2026-10-15T12:00-04:00\nA new answer \u{2192} done.\n<!-- /agent:exchange -->",
        );
        assert_eq!(committed_text_violation(&candidate, &committed), None);
    }

    #[test]
    fn duplicates_and_corruption_already_in_head_are_not_added() {
        let lossy = jna_ascii_encode(COMMITTED_RESPONSE);
        let committed = format!("{lossy}{COMMITTED_RESPONSE}{COMMITTED_RESPONSE}");
        let candidate = format!("{committed}\u{276f} next prompt\n");
        assert_eq!(committed_text_violation(&candidate, &committed), None);
    }

    #[test]
    fn short_lines_never_read_as_lossy_copies() {
        let committed = "\u{2192}\n- \u{2014} ok\n";
        let candidate = "\u{2192}\n- \u{2014} ok\n?\n- ? ok\n";
        assert_eq!(committed_text_violation(candidate, committed), None);
    }

    /// GH #274 projection shape: the editor baseline already holds the lossy
    /// copy, and the reconnect candidate replays the correct response beside it.
    #[test]
    fn gh274_projection_adding_original_beside_lossy_copy_is_refused() {
        let lossy = jna_ascii_encode(COMMITTED_RESPONSE);
        let base = format!("\u{276f} upgrade please\n{lossy}");
        let candidate = format!("{base}{COMMITTED_RESPONSE}");
        let violation = added_lossy_copy_pair(&candidate, &base).expect("refused");
        assert!(violation.candidate_line.contains("dispatched ? opus-5.5 ?"));
        assert!(
            violation
                .committed_line
                .contains("dispatched \u{2014} opus-5.5")
        );
        // The reverse direction (correct base, lossy copy added) is a pair too.
        let base = format!("\u{276f} upgrade please\n{COMMITTED_RESPONSE}");
        let candidate = format!("{base}{lossy}");
        assert!(added_lossy_copy_pair(&candidate, &base).is_some());
    }

    #[test]
    fn lossy_pair_already_in_base_and_plain_edits_pass() {
        let lossy = jna_ascii_encode(COMMITTED_RESPONSE);
        let base = format!("{lossy}{COMMITTED_RESPONSE}");
        let candidate = format!("{base}\u{276f} next prompt \u{2014} please\n");
        assert_eq!(added_lossy_copy_pair(&candidate, &base), None);
        let base = committed_doc();
        let candidate = base.replace(
            "<!-- /agent:exchange -->",
            "### Re: follow-up \u{2014} opus-5.5\nok ? fine\n?\n<!-- /agent:exchange -->",
        );
        assert_eq!(added_lossy_copy_pair(&candidate, &base), None);
    }

    fn non_ascii_line() -> impl Strategy<Value = String> {
        let piece = prop_oneof![
            "[a-z0-9]{2,6}".prop_map(|s| s),
            Just("\u{2014}".to_string()),
            Just("\u{b7}".to_string()),
            Just("\u{2192}".to_string()),
            Just("\u{2013}".to_string()),
            Just("\u{1F600}".to_string()),
            Just("\u{25CF}".to_string()),
            Just(" ".to_string()),
        ];
        proptest::collection::vec(piece, 6..24)
            .prop_map(|pieces| format!("- alpha beta {}", pieces.concat()))
            .prop_filter("needs non-ASCII", |line| !line.is_ascii())
    }

    proptest! {
        /// Any committed line carrying non-ASCII text, re-added through any of the
        /// four lossy transforms, is refused; the line re-added byte-exact is a
        /// plain duplicate line, which the line guard leaves to the block guard.
        #[test]
        fn every_lossy_rendering_of_a_committed_line_is_refused(line in non_ascii_line()) {
            let committed = format!("intro\n{line}\noutro\n");
            for rendering in lossy_renderings(&line) {
                prop_assert_ne!(&rendering, &line);
                let candidate = format!("{committed}{rendering}\n");
                let violation = committed_text_violation(&candidate, &committed);
                prop_assert_eq!(
                    violation.map(|v| v.kind),
                    Some(CommittedTextViolationKind::LossyCommittedTextCopy)
                );
            }
            let exact = format!("{committed}{line}\n");
            prop_assert_eq!(committed_text_violation(&exact, &committed), None);
        }

        /// Any lossy rendering placed beside its original in a candidate whose
        /// base held only one of the two is an added pair; with both already in
        /// the base it is not.
        #[test]
        fn every_added_lossy_pair_is_refused(line in non_ascii_line()) {
            for rendering in lossy_renderings(&line) {
                let base_original = format!("intro\n{line}\n");
                let base_lossy = format!("intro\n{rendering}\n");
                let candidate = format!("intro\n{rendering}\n{line}\n");
                prop_assert!(added_lossy_copy_pair(&candidate, &base_original).is_some());
                prop_assert!(added_lossy_copy_pair(&candidate, &base_lossy).is_some());
                prop_assert_eq!(added_lossy_copy_pair(&candidate, &candidate), None);
            }
        }
    }
}
