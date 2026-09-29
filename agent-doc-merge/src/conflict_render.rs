//! Compact, human-reconcilable rendering of a true same-span conflict between
//! the operator's text ("yours") and the agent's version (`#editorauth1` rule 3,
//! operator decision 2026-09-29: "can we have an inline conflict resolution text
//! for this if the conflict is short? Multi-line conflicts are overly verbose...
//! and long lines with a small conflict are difficult to reconcile").
//!
//! Two shapes, chosen per differing region:
//!
//! - **Inline** (short changes inside lines that correspond one-to-one): only the
//!   differing words are marked, in CriticMarkup, reading as the agent's proposed
//!   edit to your text. `{~~yours~>agent~~}` substitutes, `{++agent++}` adds,
//!   `{--yours--}` removes. Unchanged text on the line is written once.
//! - **Block** (whole-line insertions/removals, unequal line counts, or a change too
//!   long for inline): git-style markers around ONLY the differing lines, never the
//!   unchanged lines around them.
//!
//! Rendering is lossless: [`resolve_conflicts`] with [`Keep::Yours`] returns the
//! operator text byte-for-byte and [`Keep::Agent`] returns the agent text. When a
//! side already contains marker syntax, rendering refuses instead of producing text
//! that could not be resolved back.

use similar::{Algorithm, DiffTag, TextDiff, capture_diff_slices};

/// Longest single inline change (either side, in chars) before the line falls back
/// to a block.
pub const MAX_INLINE_CHANGE_CHARS: usize = 80;
/// Most inline changes on one line before it falls back to a block.
pub const MAX_INLINE_CHANGES_PER_LINE: usize = 4;

const SUB_OPEN: &str = "{~~";
const SUB_MID: &str = "~>";
const SUB_CLOSE: &str = "~~}";
const ADD_OPEN: &str = "{++";
const ADD_CLOSE: &str = "++}";
const DEL_OPEN: &str = "{--";
const DEL_CLOSE: &str = "--}";
const INLINE_TOKENS: [&str; 7] = [
    SUB_OPEN, SUB_MID, SUB_CLOSE, ADD_OPEN, ADD_CLOSE, DEL_OPEN, DEL_CLOSE,
];

const BLOCK_OPEN: &str = "<<<<<<< yours";
const BLOCK_MID: &str = "=======";
const BLOCK_CLOSE: &str = ">>>>>>> agent";
const NO_EOL: &str = "\\ No newline at end of file";
const BLOCK_LINE_PREFIXES: [&str; 4] = ["<<<<<<<", "=======", ">>>>>>>", "\\ No newline"];

/// Which side a resolution keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keep {
    /// The operator's text (the editor buffer before the conflict was surfaced).
    Yours,
    /// The agent's version.
    Agent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictRenderError {
    /// A side already contains conflict marker syntax, so a rendering could not be
    /// resolved back to it unambiguously.
    MarkerSyntaxInInput,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictResolveError {
    /// An inline or block marker was opened and never closed.
    Unterminated { offset: usize },
}

/// Render the conflict between `yours` and `agent` compactly. Returns `yours`
/// unchanged when the two are equal.
pub fn render_conflict(yours: &str, agent: &str) -> Result<String, ConflictRenderError> {
    if contains_marker_syntax(yours) || contains_marker_syntax(agent) {
        return Err(ConflictRenderError::MarkerSyntaxInInput);
    }
    let y: Vec<&str> = yours.split_inclusive('\n').collect();
    let a: Vec<&str> = agent.split_inclusive('\n').collect();
    let mut out = String::with_capacity(yours.len().max(agent.len()));
    for op in capture_diff_slices(Algorithm::Myers, &y, &a) {
        let (tag, yr, ar) = op.as_tag_tuple();
        if tag == DiffTag::Equal {
            y[yr].iter().for_each(|line| out.push_str(line));
            continue;
        }
        let (yl, al) = (&y[yr], &a[ar]);
        let inline = (yl.len() == al.len())
            .then(|| {
                yl.iter()
                    .zip(al.iter())
                    .map(|(yl, al)| render_inline_line(yl, al))
                    .collect::<Option<Vec<_>>>()
            })
            .flatten();
        match inline {
            Some(lines) => lines.iter().for_each(|line| out.push_str(line)),
            None => render_block(&mut out, yl, al),
        }
    }
    Ok(out)
}

/// True when `text` carries any inline or block conflict marker.
pub fn has_conflicts(text: &str) -> bool {
    INLINE_TOKENS.iter().any(|t| text.contains(t))
        || text
            .split_inclusive('\n')
            .any(|line| line.starts_with("<<<<<<<") || line.starts_with(">>>>>>>"))
}

/// Number of well-formed conflict marks in `text`: complete inline marks
/// (`{~~a~>b~~}`, `{++a++}`, `{--a--}` on one line) plus block openers
/// (`<<<<<<<` at a line start). Unlike [`has_conflicts`], a lone token such as
/// `i++}` or a Gemfile `~> 1.2` is not a mark.
pub fn conflict_mark_count(text: &str) -> usize {
    text.split_inclusive('\n')
        .map(|line| {
            if line.starts_with("<<<<<<<") {
                return 1;
            }
            let mut count = 0;
            let mut rest = line;
            while let Some((open, close, mid)) = next_inline_open(rest) {
                let after = &rest[open + 3..];
                let closed = match mid {
                    Some(mid) => after.find(mid).and_then(|m| {
                        after[m + mid.len()..]
                            .find(close)
                            .map(|c| m + mid.len() + c)
                    }),
                    None => after.find(close),
                };
                match closed {
                    Some(end) => {
                        count += 1;
                        rest = &after[end + close.len()..];
                    }
                    None => rest = after,
                }
            }
            count
        })
        .sum()
}

fn next_inline_open(text: &str) -> Option<(usize, &'static str, Option<&'static str>)> {
    [
        (SUB_OPEN, SUB_CLOSE, Some(SUB_MID)),
        (ADD_OPEN, ADD_CLOSE, None),
        (DEL_OPEN, DEL_CLOSE, None),
    ]
    .into_iter()
    .filter_map(|(open, close, mid)| text.find(open).map(|at| (at, close, mid)))
    .min_by_key(|(at, _, _)| *at)
}

/// True when a merge produced conflict marks that none of its inputs already
/// carried: the gate for committing merged text as resolved. Text that merely
/// quotes the notation (this repository's own session documents do) is not a
/// conflict unless the merge added one.
pub fn introduces_conflicts(merged: &str, inputs: &[&str]) -> bool {
    let carried = inputs
        .iter()
        .map(|t| conflict_mark_count(t))
        .max()
        .unwrap_or(0);
    conflict_mark_count(merged) > carried
}

/// Resolve every conflict rendered by [`render_conflict`], keeping one side.
pub fn resolve_conflicts(text: &str, keep: Keep) -> Result<String, ConflictResolveError> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut offsets = Vec::with_capacity(lines.len());
    let mut offset = 0;
    for line in &lines {
        offsets.push(offset);
        offset += line.len();
    }
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < lines.len() {
        if strip_eol(lines[i]) != BLOCK_OPEN {
            out.push_str(&resolve_inline(lines[i], keep, offsets[i])?);
            i += 1;
            continue;
        }
        let unterminated = ConflictResolveError::Unterminated { offset: offsets[i] };
        let mut j = i + 1;
        let mut ours = String::new();
        while j < lines.len() && strip_eol(lines[j]) != BLOCK_MID {
            push_block_line(&mut ours, lines[j]);
            j += 1;
        }
        if j >= lines.len() {
            return Err(unterminated);
        }
        j += 1;
        let mut theirs = String::new();
        while j < lines.len() && strip_eol(lines[j]) != BLOCK_CLOSE {
            push_block_line(&mut theirs, lines[j]);
            j += 1;
        }
        if j >= lines.len() {
            return Err(unterminated);
        }
        out.push_str(match keep {
            Keep::Yours => &ours,
            Keep::Agent => &theirs,
        });
        i = j + 1;
    }
    Ok(out)
}

fn contains_marker_syntax(text: &str) -> bool {
    INLINE_TOKENS.iter().any(|t| text.contains(t))
        || text
            .split_inclusive('\n')
            .any(|line| BLOCK_LINE_PREFIXES.iter().any(|p| line.starts_with(p)))
}

fn strip_eol(line: &str) -> &str {
    line.strip_suffix('\n').unwrap_or(line)
}

/// Render one corresponding line pair inline, or `None` when it must be a block.
fn render_inline_line(yours: &str, agent: &str) -> Option<String> {
    let (y, y_eol) = split_eol(yours);
    let (a, a_eol) = split_eol(agent);
    if y_eol != a_eol {
        return None;
    }
    let diff = TextDiff::from_words(y, a);
    // Runs of (equal text, removed text, added text); consecutive changes merge.
    let mut segments: Vec<(bool, String, String)> = Vec::new();
    for change in diff.iter_all_changes() {
        let value = change.value();
        let is_equal = change.tag() == similar::ChangeTag::Equal;
        match segments.last_mut() {
            Some((false, del, ins)) if !is_equal => match change.tag() {
                similar::ChangeTag::Delete => del.push_str(value),
                _ => ins.push_str(value),
            },
            Some((true, text, _)) if is_equal => text.push_str(value),
            _ => segments.push(match change.tag() {
                similar::ChangeTag::Equal => (true, value.to_string(), String::new()),
                similar::ChangeTag::Delete => (false, value.to_string(), String::new()),
                similar::ChangeTag::Insert => (false, String::new(), value.to_string()),
            }),
        }
    }
    // Fold a whitespace-only equal run between two changes into one change, so
    // "quick brown" -> "slow red" reads as one substitution, not two.
    let mut merged: Vec<(bool, String, String)> = Vec::new();
    let mut k = 0;
    while k < segments.len() {
        let seg = segments[k].clone();
        let folds = seg.0
            && seg.1.trim().is_empty()
            && k > 0
            && k + 1 < segments.len()
            && matches!(merged.last(), Some((false, _, _)))
            && !segments[k + 1].0;
        if folds {
            let next = &segments[k + 1];
            if let Some((_, del, ins)) = merged.last_mut() {
                del.push_str(&seg.1);
                del.push_str(&next.1);
                ins.push_str(&seg.1);
                ins.push_str(&next.2);
            }
            k += 2;
            continue;
        }
        merged.push(seg);
        k += 1;
    }
    let changes = merged.iter().filter(|s| !s.0).count();
    if changes > MAX_INLINE_CHANGES_PER_LINE {
        return None;
    }
    let mut out = String::with_capacity(yours.len() + agent.len());
    for (equal, del, ins) in &merged {
        if *equal {
            out.push_str(del);
            continue;
        }
        if del.chars().count() > MAX_INLINE_CHANGE_CHARS
            || ins.chars().count() > MAX_INLINE_CHANGE_CHARS
        {
            return None;
        }
        match (del.is_empty(), ins.is_empty()) {
            (false, false) => {
                out.push_str(SUB_OPEN);
                out.push_str(del);
                out.push_str(SUB_MID);
                out.push_str(ins);
                out.push_str(SUB_CLOSE);
            }
            (true, false) => {
                out.push_str(ADD_OPEN);
                out.push_str(ins);
                out.push_str(ADD_CLOSE);
            }
            (false, true) => {
                out.push_str(DEL_OPEN);
                out.push_str(del);
                out.push_str(DEL_CLOSE);
            }
            (true, true) => {}
        }
    }
    out.push_str(y_eol);
    Some(out)
}

fn split_eol(line: &str) -> (&str, &str) {
    match line.strip_suffix('\n') {
        Some(body) => (body, "\n"),
        None => (line, ""),
    }
}

fn render_block(out: &mut String, yours: &[&str], agent: &[&str]) {
    out.push_str(BLOCK_OPEN);
    out.push('\n');
    render_block_side(out, yours);
    out.push_str(BLOCK_MID);
    out.push('\n');
    render_block_side(out, agent);
    out.push_str(BLOCK_CLOSE);
    out.push('\n');
}

fn render_block_side(out: &mut String, lines: &[&str]) {
    for line in lines {
        out.push_str(line);
    }
    if lines.last().is_some_and(|l| !l.ends_with('\n')) {
        out.push('\n');
        out.push_str(NO_EOL);
        out.push('\n');
    }
}

/// Append a block body line; a `\ No newline` line drops the newline before it.
fn push_block_line(side: &mut String, line: &str) {
    if strip_eol(line) == NO_EOL {
        if side.ends_with('\n') {
            side.pop();
        }
    } else {
        side.push_str(line);
    }
}

fn resolve_inline(line: &str, keep: Keep, base: usize) -> Result<String, ConflictResolveError> {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    loop {
        let next = [SUB_OPEN, ADD_OPEN, DEL_OPEN]
            .iter()
            .filter_map(|open| rest.find(open).map(|at| (at, *open)))
            .min_by_key(|(at, _)| *at);
        let Some((at, open)) = next else {
            out.push_str(rest);
            return Ok(out);
        };
        out.push_str(&rest[..at]);
        let body_start = at + open.len();
        let close = match open {
            SUB_OPEN => SUB_CLOSE,
            ADD_OPEN => ADD_CLOSE,
            _ => DEL_CLOSE,
        };
        let unterminated = ConflictResolveError::Unterminated {
            offset: base + (line.len() - rest.len()) + at,
        };
        let body_len = rest[body_start..].find(close).ok_or(unterminated.clone())?;
        let body = &rest[body_start..body_start + body_len];
        let kept = match (open, keep) {
            (SUB_OPEN, _) => {
                let (del, ins) = body.split_once(SUB_MID).ok_or(unterminated)?;
                if keep == Keep::Yours { del } else { ins }
            }
            (ADD_OPEN, Keep::Agent) | (DEL_OPEN, Keep::Yours) => body,
            _ => "",
        };
        out.push_str(kept);
        rest = &rest[body_start + body_len + close.len()..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(yours: &str, agent: &str) -> String {
        let rendered = render_conflict(yours, agent).expect("renderable");
        assert_eq!(resolve_conflicts(&rendered, Keep::Yours).unwrap(), yours);
        assert_eq!(resolve_conflicts(&rendered, Keep::Agent).unwrap(), agent);
        assert_eq!(has_conflicts(&rendered), yours != agent);
        rendered
    }

    #[test]
    fn small_change_in_a_long_line_renders_inline() {
        let prefix = "The controller acknowledges the typed state, quarantines the update ";
        let yours = format!("{prefix}pending a lazy projection, then loses it.\n");
        let agent = format!("{prefix}pending a canonical projection, then loses it.\n");
        let rendered = roundtrip(&yours, &agent);
        assert_eq!(
            rendered,
            format!("{prefix}pending a {{~~lazy~>canonical~~}} projection, then loses it.\n")
        );
        assert!(!rendered.contains("<<<<<<<"));
    }

    #[test]
    fn adjacent_word_changes_merge_into_one_substitution() {
        let rendered = roundtrip("the quick brown fox\n", "the slow red fox\n");
        assert_eq!(rendered, "the {~~quick brown~>slow red~~} fox\n");
    }

    #[test]
    fn additions_and_removals_use_their_own_markers() {
        assert_eq!(
            roundtrip("keep this\n", "keep all of this\n"),
            "keep {++all of ++}this\n"
        );
        assert_eq!(
            roundtrip("keep all of this\n", "keep this\n"),
            "keep {--all of --}this\n"
        );
    }

    #[test]
    fn multi_line_conflict_marks_only_the_differing_lines() {
        let yours = "a\nb\nc one\nd\ne\n";
        let agent = "a\nb\nc two\nd\ne\n";
        assert_eq!(roundtrip(yours, agent), "a\nb\nc {~~one~>two~~}\nd\ne\n");
        let yours = "same\nmine only\nsame too\n";
        let agent = "same\nagent line one\nagent line two\nsame too\n";
        assert_eq!(
            roundtrip(yours, agent),
            "same\n<<<<<<< yours\nmine only\n=======\nagent line one\nagent line two\n>>>>>>> agent\nsame too\n"
        );
    }

    #[test]
    fn long_change_falls_back_to_a_block() {
        let yours = format!("x {} y\n", "a".repeat(MAX_INLINE_CHANGE_CHARS + 1));
        let agent = "x b y\n";
        assert!(roundtrip(&yours, agent).starts_with("<<<<<<< yours\n"));
    }

    #[test]
    fn many_scattered_changes_fall_back_to_a_block() {
        let rendered = roundtrip("a b c d e f g h i j\n", "A b C d E f G h I j\n");
        assert!(rendered.starts_with("<<<<<<< yours\n"), "{rendered}");
    }

    #[test]
    fn missing_final_newline_round_trips() {
        roundtrip("one\ntwo", "one\nthree\nfour");
        roundtrip("one\ntwo", "one\ntwo\n");
        roundtrip("", "added");
        roundtrip("removed\n", "");
        roundtrip("same", "same");
    }

    #[test]
    fn marker_syntax_in_input_is_refused() {
        assert_eq!(
            render_conflict("a {~~b~>c~~}\n", "a d\n"),
            Err(ConflictRenderError::MarkerSyntaxInInput)
        );
        assert_eq!(
            render_conflict("a\n", "=======\n"),
            Err(ConflictRenderError::MarkerSyntaxInInput)
        );
    }

    #[test]
    fn conflict_marks_are_counted_only_when_well_formed() {
        assert_eq!(
            conflict_mark_count("for (;;) { i++}\ngem 'x', '~> 1.2'\n"),
            0
        );
        assert_eq!(conflict_mark_count("a {~~b~>c~~} d {++e++} {--f--}\n"), 3);
        assert_eq!(
            conflict_mark_count("x\n<<<<<<< yours\na\n=======\nb\n>>>>>>> agent\n"),
            1
        );
        assert_eq!(conflict_mark_count("quoted `<<<<<<< yours` inline\n"), 0);
        let rendered = render_conflict("one lazy two\n", "one eager two\n").unwrap();
        assert!(introduces_conflicts(
            &rendered,
            &["one lazy two\n", "one eager two\n"]
        ));
        let quoted = "docs: `{~~yours~>agent~~}` substitutes\n";
        assert!(!introduces_conflicts(quoted, &[quoted, "docs\n"]));
    }

    #[test]
    fn unterminated_markers_are_reported() {
        assert!(resolve_conflicts("a {~~b~>c\n", Keep::Yours).is_err());
        assert!(resolve_conflicts("<<<<<<< yours\na\n=======\n", Keep::Agent).is_err());
    }

    /// Exhaustive-ish lossless check over a small vocabulary: every pair of
    /// generated documents round-trips through the rendering on both sides.
    #[test]
    fn rendering_is_lossless_for_generated_pairs() {
        let words = ["a", "bb", " ", "\n", "cc dd", "{", "~", "-", "+", "=", "é"];
        let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let gen_doc = |next: &mut dyn FnMut() -> u64| {
            let len = (next() % 12) as usize;
            (0..len)
                .map(|_| words[(next() % words.len() as u64) as usize])
                .collect::<String>()
        };
        let mut rendered = 0;
        for _ in 0..4000 {
            let yours = gen_doc(&mut next);
            let agent = gen_doc(&mut next);
            if render_conflict(&yours, &agent).is_ok() {
                roundtrip(&yours, &agent);
                rendered += 1;
            }
        }
        assert!(
            rendered > 1000,
            "only {rendered} generated pairs were renderable"
        );
    }
}
