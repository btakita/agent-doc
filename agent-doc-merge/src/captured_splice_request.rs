//! GH #326 phase 5c: the requests the editor captured-splice FFI calls
//! (`agent_doc_captured_splices_contained`, `agent_doc_rebase_captured_splices`)
//! accept, and the registration shaping they fold behind the native boundary.
//!
//! Before phase 5c the JetBrains plugin shaped a registration's texts itself
//! before asking the native proof and merge: it stripped binary-owned markers
//! (`withoutBinaryOwnedMarkersUtil`) and built the one splice from the settled
//! shadow to the live buffer (`singleSpliceBatchUtil`). A change to either rule
//! needed a plugin generation, which on the exact-263 modular package means an
//! IDE restart. A request may now carry the optional `registration` input
//! instead of a batch. Native then shapes the texts itself, so the rules below
//! change with a library hot reload.
//!
//! Compatibility: the plugin sends `registration` only to a library whose
//! `capabilities` editor derivation lists it in `captured_splice_inputs`
//! ([`CAPTURED_SPLICE_INPUTS`]). An older library gets the batch the plugin
//! shaped with its frozen copy of these rules, exactly as before. A plain batch
//! request is still answered exactly as before.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::captured_splice::{
    CapturedSplice, CapturedSpliceBatch, canonical_contains_captured, rebase,
};

/// The optional captured-splice request inputs this library understands. The
/// `capabilities` editor derivation advertises them as `captured_splice_inputs`.
/// An entry, once shipped, is never removed or renamed.
pub const CAPTURED_SPLICE_INPUTS: &[&str] = &["registration"];

/// The `registration` input: shape the batch natively from the texts a CRDT
/// replica registration holds. The FFI call's `base` argument is the settled
/// shadow and its `canonical` argument the controller canonical, both raw.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationShaping {
    /// The live editor buffer, raw. The batch is the one splice from the
    /// shadow to it.
    pub buffer: String,
    /// Compare operator text only: native removes the markers only agent-doc
    /// writes from all three texts before it shapes the splice. Asked for by
    /// the containment proof; a rebase answer is written, so it refuses this.
    #[serde(default)]
    pub operator_text: bool,
}

/// One captured-splice FFI request: a batch the caller already shaped, or the
/// `registration` input that asks native to shape it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapturedSpliceRequest {
    Batch(CapturedSpliceBatch),
    Registration(RegistrationShaping),
}

impl CapturedSpliceRequest {
    /// Parse the FFI `edits_json` argument. A non-null `registration` field
    /// wins; anything else must be a complete batch, as before phase 5c.
    pub fn parse(json: &str) -> Result<Self> {
        let mut value: serde_json::Value =
            serde_json::from_str(json).context("parse captured splice request json")?;
        let registration = value
            .as_object_mut()
            .and_then(|object| object.remove("registration"))
            .filter(|registration| !registration.is_null());
        match registration {
            Some(registration) => Ok(Self::Registration(
                serde_json::from_value(registration).context("parse registration shaping")?,
            )),
            None => Ok(Self::Batch(
                serde_json::from_value(value).context("parse captured splice batch")?,
            )),
        }
    }
}

/// `agent_doc_captured_splices_contained`: true when `canonical` already
/// carries every captured change from `base`.
pub fn contained(base: &str, canonical: &str, request: &CapturedSpliceRequest) -> Result<bool> {
    match request {
        CapturedSpliceRequest::Batch(batch) => canonical_contains_captured(base, canonical, batch),
        CapturedSpliceRequest::Registration(shaping) => {
            registration_contained(base, &shaping.buffer, canonical, shaping.operator_text)
        }
    }
}

/// `agent_doc_rebase_captured_splices`: the captured changes rebased onto
/// `canonical`, or an explicit refusal.
pub fn rebased(
    base: &str,
    canonical: &str,
    request: &CapturedSpliceRequest,
) -> Result<CapturedSpliceBatch> {
    match request {
        CapturedSpliceRequest::Batch(batch) => rebase(base, canonical, batch),
        CapturedSpliceRequest::Registration(shaping) => {
            if shaping.operator_text {
                bail!(
                    "registration operator-text shaping is for the containment proof only; \
                     a rebase answer is written text"
                );
            }
            registration_rebase(base, &shaping.buffer, canonical)
        }
    }
}

/// `#ambiguousholdforever` / `#ambiguousholdforever2`: whether the controller
/// canonical already carries every edit the operator made from the settled
/// shadow to the live buffer. With `operator_text`, the three texts are
/// compared without binary-owned markers ([`registration_operator_text`]). A
/// buffer equal to the shadow or to canonical is contained; otherwise the
/// native proof answers over the one splice from shadow to buffer.
pub fn registration_contained(
    shadow: &str,
    buffer: &str,
    canonical: &str,
    operator_text: bool,
) -> Result<bool> {
    let shaped = |text: &str| {
        if operator_text {
            registration_operator_text(text)
        } else {
            text.to_owned()
        }
    };
    let (shadow, buffer, canonical) = (shaped(shadow), shaped(buffer), shaped(canonical));
    if buffer == shadow || buffer == canonical {
        return Ok(true);
    }
    canonical_contains_captured(&shadow, &canonical, &single_splice(&shadow, &buffer))
}

/// `#editorauth2` / `#jetbrainscleanregistrati`: the clean three-way merge of a
/// registration's generations is the captured-splice rebase of the one splice
/// from the settled shadow to the live buffer, and nothing else.
pub fn registration_rebase(
    shadow: &str,
    buffer: &str,
    canonical: &str,
) -> Result<CapturedSpliceBatch> {
    rebase(shadow, canonical, &single_splice(shadow, buffer))
}

/// One splice turning `before` into `after`: the common code-point prefix and
/// suffix stay, everything between is replaced. Equal texts give no edits.
/// The JetBrains plugin's `singleSpliceBatchUtil` is the frozen copy.
pub fn single_splice(before: &str, after: &str) -> CapturedSpliceBatch {
    let old: Vec<char> = before.chars().collect();
    let new: Vec<char> = after.chars().collect();
    let mut prefix = 0;
    while prefix < old.len() && prefix < new.len() && old[prefix] == new[prefix] {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < old.len() - prefix
        && suffix < new.len() - prefix
        && old[old.len() - 1 - suffix] == new[new.len() - 1 - suffix]
    {
        suffix += 1;
    }
    let delete = old.len() - prefix - suffix;
    let insert: String = new[prefix..new.len() - suffix].iter().collect();
    let edits = if delete == 0 && insert.is_empty() {
        Vec::new()
    } else {
        vec![CapturedSplice {
            offset_code_points: prefix,
            delete_code_points: delete,
            insert,
        }]
    };
    CapturedSpliceBatch {
        edits,
        resulting_text: after.to_owned(),
    }
}

/// `#ambiguousholdforever2`: `text` without the markers only agent-doc writes,
/// as registration compares operator text. This is the rule the JetBrains
/// plugin's `withoutBinaryOwnedMarkersUtil` applied, byte for byte, including
/// its Java regex line semantics:
///
/// 1. Remove each boundary-marker line: optional spaces and tabs, then
///    `<!-- agent:boundary:ID -->` with `ID` matching `[a-z0-9][a-z0-9:-]*`,
///    then optional spaces and tabs. A `\n` or `\r\n` ending the line is
///    removed with it; any other line terminator stays.
/// 2. Then, on the result, drop a trailing ` (HEAD)` (and spaces or tabs after
///    it) from an ATX heading line: one to six `#` at the start of the line, a
///    space, and at least one more character before the ` (HEAD)`.
///
/// Lines end at `\n`, `\r\n`, `\r`, U+0085, U+2028, or U+2029, as in
/// `java.util.regex` multiline mode. This differs from
/// [`crate::captured_splice::without_binary_owned_markers`], which the
/// controller's retained-write proof uses; the registration rule is kept
/// exactly so a plugin and a library compare the same operator text.
pub fn registration_operator_text(text: &str) -> String {
    let without_boundaries = map_java_lines(text, |body, terminator| {
        if is_boundary_marker_line(body) {
            // `(?:\r?\n|$)`: a `\n` or `\r\n` is consumed with the line; `$`
            // leaves any other terminator in place.
            match terminator {
                "\n" | "\r\n" => LineEdit::Remove,
                _ => LineEdit::Body(""),
            }
        } else {
            LineEdit::Keep
        }
    });
    map_java_lines(&without_boundaries, |body, _| {
        match head_suffix_start(body) {
            Some(end) => LineEdit::Body(&body[..end]),
            None => LineEdit::Keep,
        }
    })
}

enum LineEdit<'a> {
    /// Keep the line and its terminator.
    Keep,
    /// Replace the line's body, keeping its terminator.
    Body(&'a str),
    /// Drop the line and its terminator.
    Remove,
}

/// Rebuild `text` line by line with Java multiline line terminators.
fn map_java_lines<'a>(
    text: &'a str,
    mut edit: impl FnMut(&'a str, &'a str) -> LineEdit<'a>,
) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        let body_end = rest
            .char_indices()
            .find(|(_, ch)| is_java_line_terminator(*ch))
            .map_or(rest.len(), |(index, _)| index);
        let (body, after) = rest.split_at(body_end);
        let terminator_len = if after.starts_with("\r\n") {
            2
        } else {
            after.chars().next().map_or(0, char::len_utf8)
        };
        let (terminator, next) = after.split_at(terminator_len);
        match edit(body, terminator) {
            LineEdit::Keep => {
                out.push_str(body);
                out.push_str(terminator);
            }
            LineEdit::Body(replacement) => {
                out.push_str(replacement);
                out.push_str(terminator);
            }
            LineEdit::Remove => {}
        }
        rest = next;
    }
    out
}

/// The characters `java.util.regex` ends a line at outside `UNIX_LINES` mode;
/// `.` matches none of them.
fn is_java_line_terminator(ch: char) -> bool {
    matches!(ch, '\n' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

/// `[ \t]*<!-- agent:boundary:[a-z0-9][a-z0-9:-]* -->[ \t]*`, anchored to the
/// whole line.
fn is_boundary_marker_line(body: &str) -> bool {
    let core = body.trim_matches([' ', '\t']);
    let Some(id) = core
        .strip_prefix("<!-- agent:boundary:")
        .and_then(|rest| rest.strip_suffix(" -->"))
    else {
        return false;
    };
    let mut chars = id.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_lowercase() || first.is_ascii_digit())
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == ':' || ch == '-')
}

/// `^(#{1,6} .*?) \(HEAD\)[ \t]*$` over one line: the byte length of group 1
/// when the line matches.
fn head_suffix_start(body: &str) -> Option<usize> {
    let hashes = body.bytes().take_while(|byte| *byte == b'#').count();
    if !(1..=6).contains(&hashes) || body.as_bytes().get(hashes) != Some(&b' ') {
        return None;
    }
    let start = body
        .trim_end_matches([' ', '\t'])
        .strip_suffix(" (HEAD)")?
        .len();
    (start > hashes).then_some(start)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(offset: usize, delete: usize, insert: &str) -> CapturedSplice {
        CapturedSplice {
            offset_code_points: offset,
            delete_code_points: delete,
            insert: insert.into(),
        }
    }

    #[test]
    fn single_splice_keeps_the_common_code_point_prefix_and_suffix() {
        let batch = single_splice("a🌍b", "a🌍xb");
        assert_eq!(batch.edits, vec![edit(2, 0, "x")]);
        assert_eq!(batch.resulting_text, "a🌍xb");
        assert!(single_splice("same", "same").edits.is_empty());
        // Overlapping prefix and suffix never double-count a code point.
        assert_eq!(single_splice("aa", "aaa").edits, vec![edit(2, 0, "a")]);
        assert_eq!(single_splice("aaa", "aa").edits, vec![edit(2, 1, "")]);
    }

    #[test]
    fn registration_operator_text_follows_the_java_regex_rule() {
        let cases = [
            ("text\n<!-- agent:boundary:ab12 -->", "text\n"),
            ("a\n<!-- agent:boundary:ab12 -->\r\nb", "a\nb"),
            ("a\n \t<!-- agent:boundary:ab-1:x -->\t \nb", "a\nb"),
            // Not binary-owned: uppercase or empty id, inline, wrong spacing.
            (
                "<!-- agent:boundary:AB -->\n",
                "<!-- agent:boundary:AB -->\n",
            ),
            ("<!-- agent:boundary: -->\n", "<!-- agent:boundary: -->\n"),
            (
                "x <!-- agent:boundary:ab -->\n",
                "x <!-- agent:boundary:ab -->\n",
            ),
            (
                "<!--  agent:boundary:ab -->\n",
                "<!--  agent:boundary:ab -->\n",
            ),
            // A terminator other than `\n` / `\r\n` stays.
            ("<!-- agent:boundary:ab -->\rx", "\rx"),
            ("<!-- agent:boundary:ab -->\u{2028}x", "\u{2028}x"),
            ("## Re: x (HEAD)\n", "## Re: x\n"),
            ("## Re: x (HEAD) \t\r\n", "## Re: x\r\n"),
            ("## Re: x (HEAD)\u{85}y", "## Re: x\u{85}y"),
            ("## a (HEAD) (HEAD)\n", "## a (HEAD)\n"),
            ("####### x (HEAD)\n", "####### x (HEAD)\n"),
            (" ## x (HEAD)\n", " ## x (HEAD)\n"),
            ("##x (HEAD)\n", "##x (HEAD)\n"),
            ("# (HEAD)\n", "# (HEAD)\n"),
            ("#  (HEAD)\n", "# \n"),
            (
                "### Re: topic (HEAD) was renamed\n",
                "### Re: topic (HEAD) was renamed\n",
            ),
            // The boundary pass runs first: removing it can join `\r` and `\n`.
            ("## h (HEAD)\r<!-- agent:boundary:ab -->\n\n", "## h\r\n"),
        ];
        for (input, expected) in cases {
            assert_eq!(registration_operator_text(input), expected, "{input:?}");
        }
    }

    #[test]
    fn requests_parse_as_a_batch_or_as_registration_shaping() {
        let batch = r#"{"edits":[{"offsetCodePoints":1,"deleteCodePoints":0,"insert":"x"}],"resultingText":"axb"}"#;
        assert!(matches!(
            CapturedSpliceRequest::parse(batch).unwrap(),
            CapturedSpliceRequest::Batch(_)
        ));
        let null_registration = r#"{"edits":[],"resultingText":"a","registration":null}"#;
        assert!(matches!(
            CapturedSpliceRequest::parse(null_registration).unwrap(),
            CapturedSpliceRequest::Batch(_)
        ));
        let registration = r#"{"registration":{"buffer":"axb","operatorText":true}}"#;
        assert_eq!(
            CapturedSpliceRequest::parse(registration).unwrap(),
            CapturedSpliceRequest::Registration(RegistrationShaping {
                buffer: "axb".into(),
                operator_text: true,
            })
        );
        // An older library rejected a request without a batch; this one does
        // too unless it carries `registration`.
        assert!(CapturedSpliceRequest::parse(r#"{"buffer":"x"}"#).is_err());
    }

    #[test]
    fn a_rebase_never_answers_over_operator_text() {
        let request = CapturedSpliceRequest::Registration(RegistrationShaping {
            buffer: "b".into(),
            operator_text: true,
        });
        assert!(rebased("a", "c", &request).is_err());
    }

    /// The shared Rust/Kotlin parity file. The plugin's frozen floor
    /// (`withoutBinaryOwnedMarkersUtil`, `singleSpliceBatchUtil`) answers the
    /// same file in `RegistrationSpliceShapingTest`, so the two rules cannot
    /// drift silently. Each case is also answered through the pre-5c batch
    /// path with the expected shaped batch, so an older plugin beside this
    /// library and a 5c plugin get the same answer.
    #[test]
    fn gh326_registration_shaping_answers_the_shared_parity_cases() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../editors/jetbrains/src/test/resources/editor-derivations/registration_splice_shaping.json"
        ))
        .unwrap();
        let normalization = fixture["operator_text"].as_array().unwrap();
        assert!(
            normalization.len() >= 20,
            "the parity file lost normalization cases"
        );
        for case in normalization {
            let name = case["name"].as_str().unwrap();
            assert_eq!(
                registration_operator_text(case["text"].as_str().unwrap()),
                case["expected"].as_str().unwrap(),
                "{name}"
            );
        }
        let cases = fixture["registrations"].as_array().unwrap();
        assert!(cases.len() >= 10, "the parity file lost registration cases");
        for case in cases {
            let name = case["name"].as_str().unwrap();
            let shadow = case["shadow"].as_str().unwrap();
            let buffer = case["buffer"].as_str().unwrap();
            let canonical = case["canonical"].as_str().unwrap();
            let expected = &case["expected"];
            let operator = |text: &str| registration_operator_text(text);
            assert_eq!(
                operator(shadow),
                expected["operator_shadow"].as_str().unwrap(),
                "{name}"
            );
            assert_eq!(
                operator(buffer),
                expected["operator_buffer"].as_str().unwrap(),
                "{name}"
            );
            assert_eq!(
                operator(canonical),
                expected["operator_canonical"].as_str().unwrap(),
                "{name}"
            );

            let containment_batch = single_splice(&operator(shadow), &operator(buffer));
            assert_eq!(
                serde_json::to_value(&containment_batch).unwrap(),
                expected["containment_batch"],
                "{name}"
            );
            let request = |buffer: &str, operator_text: bool| {
                CapturedSpliceRequest::parse(
                    &serde_json::json!({
                        "registration": {"buffer": buffer, "operatorText": operator_text}
                    })
                    .to_string(),
                )
                .unwrap()
            };
            let folded = contained(shadow, canonical, &request(buffer, true)).unwrap();
            assert_eq!(folded, expected["contained"].as_bool().unwrap(), "{name}");
            // The pre-5c path: the plugin's shortcut, then the batch it shaped.
            let shortcut = expected["containment_shortcut"].as_bool().unwrap();
            assert_eq!(
                shortcut,
                operator(buffer) == operator(shadow) || operator(buffer) == operator(canonical),
                "{name}"
            );
            if !shortcut {
                let batch =
                    CapturedSpliceRequest::parse(&expected["containment_batch"].to_string())
                        .unwrap();
                assert_eq!(
                    contained(&operator(shadow), &operator(canonical), &batch).unwrap(),
                    folded,
                    "{name}: the pre-5c batch path must agree"
                );
            }

            let merge_batch = single_splice(shadow, buffer);
            assert_eq!(
                serde_json::to_value(&merge_batch).unwrap(),
                expected["merge_batch"],
                "{name}"
            );
            let folded_merge = rebased(shadow, canonical, &request(buffer, false))
                .ok()
                .map(|batch| batch.resulting_text);
            assert_eq!(
                folded_merge.as_deref(),
                expected["merged"].as_str(),
                "{name}"
            );
            let batch = CapturedSpliceRequest::parse(&expected["merge_batch"].to_string()).unwrap();
            assert_eq!(
                rebased(shadow, canonical, &batch)
                    .ok()
                    .map(|batch| batch.resulting_text),
                folded_merge,
                "{name}: the pre-5c batch path must agree"
            );
        }
    }
}
