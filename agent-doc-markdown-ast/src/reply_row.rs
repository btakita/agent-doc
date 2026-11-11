//! Binary-owned reply rows inside a Waiting-on-you packet (`#barereplymarker`).
//!
//! agent-doc renders each generated packet row (`**Next:**`, its nested
//! `**Roll-up:**` / `**Subtree:**` triage rows, `**Response:**`,
//! `**Timestamp:**`, `**Question:**`, `**Choices:**`, offered choices,
//! `**Next actions:**`, `**Action:**`, ...) with a trailing
//! [`AGENT_DOC_REPLY_GLYPH`]. That glyph is the row's only ownership mark.
//!
//! Older documents also carried a hidden `<!-- agent-doc:reply -->` (or, before
//! that, `<!-- agent-doc:waiting-metadata -->`) comment. Those namespaced
//! comments still prove ownership wherever they appear, so an un-migrated
//! document parses exactly as before; the next refresh rewrites such a row to
//! the bare-glyph form ([`bare_reply_row_body`]).
//!
//! ## The operator-text rule
//!
//! A bare glyph proves ownership only in the rendered position. A row is a
//! binary-owned reply row when **all** of these hold:
//!
//! 1. the glyph is the row's last whitespace-separated token, with content
//!    before it (a leading or mid-line `▫️` is ordinary text). A generated
//!    `[#q-…]` written after the glyph does not move that end: readers still
//!    see the glyph, and the next refresh strips the annotation
//!    (`#responserowclaim`);
//! 2. its parent row is a binary-owned packet row: the `<!-- agent-doc:waiting-item -->`
//!    item, the Waiting-on-you root (`<!-- agent-doc:waiting-on-you -->`), or
//!    another reply row (recursively, by this same rule);
//! 3. its content starts with one of the generated labels in
//!    [`REPLY_ROW_LABELS`], unless its parent is the generated `**Choices:**`
//!    row, whose children are free-text offered choices.
//!
//! Anywhere else (top-level queue rows, rows outside the Waiting-on-you tree,
//! an unlabeled line typed under an item, a choice, or a `**Response:**` row) a
//! trailing `▫️` is operator text and stays operator text, so it can never be
//! regenerated away by a packet refresh. The only operator text the rule can
//! claim is a line that reproduces a generated row's exact rendered shape in
//! its rendered position, which is the reserved syntax this glyph names in the
//! legend. Nothing is appended after the glyph. A generated Next, Question,
//! Response, or Action row is never queue work, including when its item head
//! is missing and the row sits under the root.
//! A row whose entire body is only `▫️` still is not a binary-owned reply row,
//! but it carries no decision payload; the waiting-packet classifier ignores
//! that empty artifact instead of treating it as an operator answer.

use crate::overlay::{split_struck_body, strip_bullet};

pub use agent_doc_queue_icons::AGENT_DOC_REPLY_GLYPH;

/// Legacy hidden ownership comment (accepted, never rendered).
pub const LEGACY_REPLY_MARKER: &str = "<!-- agent-doc:reply -->";
/// Older legacy ownership comment (accepted, never rendered).
pub const LEGACY_WAITING_METADATA_MARKER: &str = "<!-- agent-doc:waiting-metadata -->";
/// Marker on the head row of a generated Waiting-on-you item.
pub const WAITING_ITEM_MARKER: &str = "<!-- agent-doc:waiting-item -->";
/// Marker on the Waiting-on-you root. Packet rows whose item head is missing
/// still belong to this root (`#responserowclaim`).
pub const WAITING_ON_YOU_ROOT_MARKER: &str = "<!-- agent-doc:waiting-on-you -->";

/// Labels that open a generated (non-choice) packet row.
pub const REPLY_ROW_LABELS: [&str; 11] = [
    "**Next:**",
    "**Roll-up:**",
    "**Subtree:**",
    "**Response:**",
    "**Timestamp:**",
    "**Context:**",
    "**Question:**",
    "**Recommendation:**",
    "**Choices:**",
    "**Next actions:**",
    "**Action:**",
];

const CHOICES_LABEL: &str = "**Choices:**";

/// One row of a tree being classified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyRow<'a> {
    /// A blank line; skipped when looking for a parent.
    Blank,
    /// A non-list line; it ends the parent search.
    Other { indent: usize },
    /// A list item and its body (the text after the bullet).
    Item { indent: usize, body: &'a str },
}

/// Ordered rows of a list tree. Implemented for markdown lines here and for
/// parsed queue entries in `agent-doc-queue`, so both read reply ownership
/// through [`is_agent_doc_reply_row_in`].
pub trait ReplyRows {
    fn row_count(&self) -> usize;
    fn row(&self, index: usize) -> ReplyRow<'_>;
}

/// Classify one markdown source line.
pub fn reply_row_of_line(line: &str) -> ReplyRow<'_> {
    let line = line.trim_end_matches(['\n', '\r']);
    if line.trim().is_empty() {
        return ReplyRow::Blank;
    }
    let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
    match strip_bullet(line) {
        Some(body) => ReplyRow::Item { indent, body },
        None => ReplyRow::Other { indent },
    }
}

impl<S: AsRef<str>> ReplyRows for [S] {
    fn row_count(&self) -> usize {
        self.len()
    }

    fn row(&self, index: usize) -> ReplyRow<'_> {
        self.get(index)
            .map_or(ReplyRow::Blank, |line| reply_row_of_line(line.as_ref()))
    }
}

/// Whether `text` carries a legacy namespaced ownership comment.
pub fn has_legacy_reply_marker(text: &str) -> bool {
    text.contains(LEGACY_REPLY_MARKER) || text.contains(LEGACY_WAITING_METADATA_MARKER)
}

/// The row's visible content with every ownership mark removed: legacy
/// comments, a trailing generated `[#q-…]`, and the trailing glyph. Used to
/// read a generated row's value.
pub fn reply_row_text(body: &str) -> String {
    let (_, inner) = split_struck_body(body.trim());
    let without_comments = inner
        .replace(LEGACY_REPLY_MARKER, "")
        .replace(LEGACY_WAITING_METADATA_MARKER, "");
    let trimmed = without_trailing_generated_qid(without_comments.trim());
    trimmed
        .strip_suffix(AGENT_DOC_REPLY_GLYPH)
        .unwrap_or(trimmed)
        .trim()
        .to_string()
}

/// The canonical rendering of a generated row body: its content followed by
/// the trailing glyph, with legacy comments and strike wrappers removed.
pub fn bare_reply_row_body(body: &str) -> String {
    format!("{} {AGENT_DOC_REPLY_GLYPH}", reply_row_text(body))
}

/// Content before a trailing glyph token, when the glyph is in the rendered
/// position (last token, preceded by whitespace and some content). A generated
/// `[#q-…]` after the glyph is ignored (`#responserowclaim`).
fn trailing_glyph_content(body: &str) -> Option<&str> {
    let (_, inner) = split_struck_body(body.trim());
    let before_glyph = without_trailing_generated_qid(inner.trim_end());
    let before = before_glyph.strip_suffix(AGENT_DOC_REPLY_GLYPH)?;
    if !before.ends_with(char::is_whitespace) {
        return None;
    }
    let content = before.trim_end();
    (!content.is_empty()).then_some(content)
}

/// Whether `body` has the generated reply-label shape, ignoring its parent.
///
/// Stable-id admission uses this so it never writes a `[#q-…]` after the
/// glyph. Parent position still decides whether the row is binary-owned.
pub fn is_generated_reply_label_body(body: &str) -> bool {
    let Some(content) = trailing_glyph_content(body) else {
        return false;
    };
    starts_with_reply_label(content)
}

fn without_trailing_generated_qid(text: &str) -> &str {
    let mut current = text;
    while let Some(stripped) = strip_one_trailing_generated_qid(current) {
        current = stripped;
    }
    current
}

fn strip_one_trailing_generated_qid(text: &str) -> Option<&str> {
    let (work, annotation) = text.rsplit_once(" [#")?;
    let id = annotation.strip_suffix(']')?;
    if !is_generated_qid(id) || !work.ends_with(AGENT_DOC_REPLY_GLYPH) {
        return None;
    }
    Some(work)
}

fn is_generated_qid(id: &str) -> bool {
    let Some(rest) = id.strip_prefix("q-") else {
        return false;
    };
    !rest.is_empty()
        && rest
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
}

/// Drop a trailing generated `[#q-…]` from one source line when it sits
/// immediately after the reply glyph, leaving the line's newline in place.
pub fn strip_generated_reply_qid_line(line: &str) -> String {
    let core_end = line.trim_end_matches(['\n', '\r']).len();
    let core = &line[..core_end];
    let newline = &line[core_end..];
    match strip_generated_reply_qid_core(core) {
        Some(stripped) => format!("{stripped}{newline}"),
        None => line.to_string(),
    }
}

fn strip_generated_reply_qid_core(core: &str) -> Option<String> {
    let (head, closer) = if let Some(head) = core.strip_suffix("~~") {
        (head, "~~")
    } else {
        (core, "")
    };
    let head = head.trim_end();
    let (work, annotation) = head.rsplit_once(" [#")?;
    let id = annotation.strip_suffix(']')?;
    if !is_generated_qid(id) || !work.ends_with(AGENT_DOC_REPLY_GLYPH) {
        return None;
    }
    Some(format!("{work}{closer}"))
}

/// Generated content with any lifecycle projection accidentally painted onto
/// the row (`📥 **Response:** …`) removed; the projection repair
/// (`normalize_generated_response_lifecycle_markers`) strips it from the bytes.
fn generated_content(content: &str) -> &str {
    agent_doc_element_queue::strip_attempt_projection_prefix(content)
}

fn starts_with_reply_label(content: &str) -> bool {
    let content = generated_content(content);
    REPLY_ROW_LABELS
        .iter()
        .any(|label| content.starts_with(label))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Parent {
    WaitingItem,
    Reply { choices: bool },
    Other,
}

fn parent_of<R: ReplyRows + ?Sized>(rows: &R, index: usize, indent: usize) -> Parent {
    for candidate in (0..index).rev() {
        match rows.row(candidate) {
            ReplyRow::Blank => continue,
            ReplyRow::Other { indent: other } | ReplyRow::Item { indent: other, .. }
                if other >= indent =>
            {
                continue;
            }
            ReplyRow::Other { .. } => return Parent::Other,
            ReplyRow::Item { body, .. } => {
                if body.contains(WAITING_ITEM_MARKER) || body.contains(WAITING_ON_YOU_ROOT_MARKER) {
                    return Parent::WaitingItem;
                }
                if is_agent_doc_reply_row_in(rows, candidate) {
                    return Parent::Reply {
                        choices: generated_content(&reply_row_text(body)) == CHOICES_LABEL,
                    };
                }
                return Parent::Other;
            }
        }
    }
    Parent::Other
}

/// THE reply-row predicate: whether row `index` of `rows` is a binary-owned
/// reply row. See the module docs for the rule.
pub fn is_agent_doc_reply_row_in<R: ReplyRows + ?Sized>(rows: &R, index: usize) -> bool {
    let ReplyRow::Item { indent, body } = rows.row(index) else {
        return false;
    };
    if has_legacy_reply_marker(body) {
        return true;
    }
    let Some(content) = trailing_glyph_content(body) else {
        return false;
    };
    match parent_of(rows, index, indent) {
        Parent::WaitingItem | Parent::Reply { choices: false } => starts_with_reply_label(content),
        Parent::Reply { choices: true } => true,
        Parent::Other => false,
    }
}

/// [`is_agent_doc_reply_row_in`] over markdown lines.
pub fn is_agent_doc_reply_row_at(lines: &[&str], index: usize) -> bool {
    is_agent_doc_reply_row_in(lines, index)
}

/// Rewrite legacy reply rows (hidden comment, strike wrapper) to the bare
/// trailing-glyph form, but only where the bare form is still recognized in
/// that position. A legacy row outside a waiting packet keeps a canonical
/// `▫️ <!-- agent-doc:reply -->` suffix so it never loses its ownership proof.
/// Every other line, including operator text, is returned byte-for-byte.
pub fn normalize_reply_rows(source: &str) -> String {
    let mut lines = source
        .split_inclusive('\n')
        .map(str::to_string)
        .collect::<Vec<_>>();
    for index in 0..lines.len() {
        let original = lines[index].clone();
        let ReplyRow::Item { body, .. } = reply_row_of_line(&original) else {
            continue;
        };
        if !has_legacy_reply_marker(body) {
            continue;
        }
        let core_end = original.trim_end_matches(['\n', '\r']).len();
        let newline = &original[core_end..];
        let body_offset = body.as_ptr() as usize - original.as_ptr() as usize;
        let prefix = &original[..body_offset];
        let body = &original[body_offset..core_end];
        lines[index] = format!("{prefix}{}{newline}", bare_reply_row_body(body));
        if !is_agent_doc_reply_row_in(lines.as_slice(), index) {
            lines[index] = format!(
                "{prefix}{} {LEGACY_REPLY_MARKER}{newline}",
                bare_reply_row_body(body)
            );
        }
    }
    // `#responserowclaim`: a generated `[#q-…]` after the glyph is not the
    // row's id. Strip it only where the row is still a reply row, so an
    // operator prompt that happens to end in the glyph keeps its own id.
    for index in 0..lines.len() {
        if !is_agent_doc_reply_row_in(lines.as_slice(), index) {
            continue;
        }
        let original = lines[index].clone();
        let stripped = strip_generated_reply_qid_line(&original);
        if stripped == original {
            continue;
        }
        lines[index] = stripped;
        if !is_agent_doc_reply_row_in(lines.as_slice(), index) {
            lines[index] = original;
        }
    }
    lines.concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(source: &str) -> Vec<&str> {
        source.lines().collect()
    }

    fn reply_flags(source: &str) -> Vec<bool> {
        let rows = lines(source);
        (0..rows.len())
            .map(|index| is_agent_doc_reply_row_at(&rows, index))
            .collect()
    }

    const PACKET: &str = concat!(
        "- ### Waiting on you <!-- agent-doc:waiting-on-you -->\n",
        "  - [#pick] Pick a port. <!-- agent-doc:waiting-item -->\n",
        "    - **Next:** you — answer: Which port? (evidence: Question) ▫️\n",
        "    - **Response:** Pick a port. ▫️\n",
        "      - **Timestamp:** 2026-11-04T10:00-05:00 ▫️\n",
        "      - **Question:** Which port? ▫️\n",
        "      - **Choices:** ▫️\n",
        "        - **8080 (Recommended)**: Default. ▫️\n",
        "        - Commit only ▫️\n",
        "      - **Next actions:** ▫️\n",
        "        - **Action:** Pick a port. ▫️\n",
    );

    #[test]
    fn bare_trailing_glyph_rows_in_a_packet_are_binary_owned() {
        assert_eq!(
            reply_flags(PACKET),
            vec![
                false, false, true, true, true, true, true, true, true, true, true
            ]
        );
    }

    #[test]
    fn operator_text_with_the_glyph_is_never_binary_owned() {
        // Operator lines typed into a packet, each ending in the glyph.
        let source = format!(
            "{PACKET}{}",
            concat!(
                // Unlabeled answers under a generated Action and Response row.
                "          - yes, this one ▫️\n",
                "      - sounds good ▫️\n",
                // Unlabeled answer directly under the item.
                "    - use 8080 ▫️\n",
                // A labeled-looking row under an operator line is still theirs.
                "      - **Question:** under my own line ▫️\n",
                // Leading and mid-line glyphs never count.
                "    - ▫️ **Response:** leading glyph\n",
                "    - **Question:** mid ▫️ line\n",
                // A bare glyph row has no content.
                "    - ▫️\n",
                // Glyph glued to the content is not the rendered token.
                "    - **Response:** glued▫️\n",
            )
        );
        let flags = reply_flags(&source);
        let rows = lines(&source);
        for (index, row) in rows.iter().enumerate().skip(11) {
            assert!(!flags[index], "operator row claimed as generated: {row}");
        }
        // The generated rows above them are unaffected.
        assert!(flags[2..11].iter().all(|flag| *flag), "{flags:?}");

        // Outside a waiting packet a trailing glyph is ordinary operator text,
        // even with a generated label.
        let top_level = concat!(
            "- **Response:** an operator note ▫️\n",
            "  - **Question:** still mine ▫️\n",
            "- plain work ▫️\n",
            "  - [#x] not a waiting item\n",
            "    - **Response:** mine too ▫️\n",
        );
        assert_eq!(reply_flags(top_level), vec![false; 5]);
    }

    #[test]
    fn legacy_comment_rows_stay_recognized_anywhere() {
        let source = concat!(
            "- **Response:** Choose a target. ▫️ <!-- agent-doc:reply -->\n",
            "- **Timestamp:** now <!-- agent-doc:reply -->\n",
            "- ~~**Response:** old <!-- agent-doc:waiting-metadata -->~~\n",
            "- ▫️ operator-authored choice\n",
        );
        assert_eq!(reply_flags(source), vec![true, true, true, false]);
    }

    #[test]
    fn normalization_drops_the_comment_inside_a_packet_only() {
        let legacy = concat!(
            "  - [#pick] Pick a port. <!-- agent-doc:waiting-item -->\n",
            "    - **Response:** Pick a port. ▫️ <!-- agent-doc:reply -->\n",
            "      - **Choices:** <!-- agent-doc:reply -->\n",
            "        - Commit only ▫️ <!-- agent-doc:reply -->\n",
            "          - operator child stays byte-stable ▫️\n",
            "    - ~~**Question:** old <!-- agent-doc:waiting-metadata -->~~\n",
        );
        let normalized = normalize_reply_rows(legacy);
        assert_eq!(
            normalized,
            concat!(
                "  - [#pick] Pick a port. <!-- agent-doc:waiting-item -->\n",
                "    - **Response:** Pick a port. ▫️\n",
                "      - **Choices:** ▫️\n",
                "        - Commit only ▫️\n",
                "          - operator child stays byte-stable ▫️\n",
                "    - **Question:** old ▫️\n",
            )
        );
        assert!(!normalized.contains("agent-doc:reply"));
        assert_eq!(normalize_reply_rows(&normalized), normalized);
        let rows = lines(&normalized);
        assert!(is_agent_doc_reply_row_at(&rows, 3));
        assert!(!is_agent_doc_reply_row_at(&rows, 4));

        // Outside a packet the bare form would lose ownership, so the comment
        // stays (canonicalized) and the row stays binary-owned.
        let outside = "- **Response:** Choose. <!-- agent-doc:waiting-metadata -->\n";
        let normalized = normalize_reply_rows(outside);
        assert_eq!(
            normalized,
            "- **Response:** Choose. ▫️ <!-- agent-doc:reply -->\n"
        );
        assert_eq!(reply_flags(&normalized), vec![true]);

        let operator = "  - ▫️ operator-authored queue work\n";
        assert_eq!(normalize_reply_rows(operator), operator);
    }

    #[test]
    fn reply_row_text_strips_every_ownership_mark() {
        assert_eq!(
            reply_row_text("**Question:** Who? ▫️"),
            "**Question:** Who?"
        );
        assert_eq!(
            reply_row_text("**Question:** Who? ▫️ <!-- agent-doc:reply -->"),
            "**Question:** Who?"
        );
        assert_eq!(
            reply_row_text("~~**Question:** Who? <!-- agent-doc:waiting-metadata -->~~"),
            "**Question:** Who?"
        );
        assert_eq!(
            reply_row_text("**Question:** Who? ▫️ [#q-095ecb037967]"),
            "**Question:** Who?"
        );
    }

    #[test]
    fn a_trailing_generated_qid_does_not_end_ownership_under_the_root() {
        let source = concat!(
            "- ### Waiting on you <!-- agent-doc:waiting-on-you -->\n",
            "    - 📥 **Next:** you — answer: Which port? (evidence: subagent:tcoreproof-moot#1) ▫️ [#q-bfc40eeaae02]\n",
            "    - 📥 **Question:** Which port? ▫️ [#q-095ecb037967]\n",
            "    - **Response:** Pick a port. ▫️ [#q-aaaaaaaaaaaa]\n",
            "    - **Next actions:** ▫️\n",
            "      - 📥 **Action:** Pick a port. ▫️\n",
            "    - **Choices:** ▫️\n",
            "      - **8080 (Recommended)**: Default. ▫️ [#q-bbbbbbbbbbbb]\n",
            "      - Commit only ▫️ [#q-cccccccccccc]\n",
            "      - use 8080\n",
            "- **Response:** an operator note ▫️\n",
            "- fix the flaky test ▫️ [#q-dddddddddddd]\n",
        );
        let flags = reply_flags(source);
        let rows = lines(source);
        let owned_flags = [
            false, true, true, true, true, true, true, true, true, false, false, false,
        ];
        assert_eq!(flags.len(), owned_flags.len(), "{rows:#?}");
        for (index, owned) in owned_flags.into_iter().enumerate() {
            assert_eq!(
                flags[index], owned,
                "row {index} ({}) owned={owned} flags={flags:?}",
                rows[index]
            );
        }
        assert!(is_generated_reply_label_body(
            "📥 **Action:** Pick a port. ▫️ [#q-eeeeeeeeeeee]"
        ));
        assert!(!is_generated_reply_label_body(
            "use 8080 ▫️ [#q-dddddddddddd]"
        ));
        assert!(!is_generated_reply_label_body(
            "**Response:** an operator note ▫️ extra"
        ));

        let normalized = normalize_reply_rows(source);
        for label in [
            "**Next:**",
            "**Question:**",
            "**Response:** Pick",
            "**Action:**",
            "**8080 (Recommended)**",
            "Commit only",
        ] {
            let row = normalized
                .lines()
                .find(|line| line.contains(label))
                .unwrap_or_else(|| panic!("missing {label} in {normalized}"));
            assert!(
                row.trim_end().ends_with(AGENT_DOC_REPLY_GLYPH),
                "row must end at the glyph: {row}"
            );
            assert!(!row.contains("[#q-"), "{row}");
        }
        assert!(normalized.contains("      - use 8080\n"), "{normalized}");
        assert!(
            normalized.contains("- fix the flaky test ▫️ [#q-dddddddddddd]\n"),
            "an unlabeled operator id stays: {normalized}"
        );
        assert!(
            normalized.contains("- **Response:** an operator note ▫️\n"),
            "{normalized}"
        );
        assert_eq!(normalize_reply_rows(&normalized), normalized);
    }
}
