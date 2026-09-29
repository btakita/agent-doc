//! Reconcile the operator's live edit with the agent's incoming version of the
//! same document (`#editorauth4`, plan: tasks/agent-doc/plan-editor-authority-ladder.md).
//!
//! This is the text implementation of the merge proved in
//! `formal/authority_ladder/ConflictReconciliation.lean` (`#editorauth1`) and
//! model-checked in `formal/tla/ConflictReconciliation.tla`. Both sides are
//! diffed against the shared `base` into edits (replace `[start, end)` of the
//! base with `ins`), and each pair of edits follows the operator's rules:
//!
//! 1. **Same-point appends** (both edits are pure insertions at one offset): the
//!    agent's text goes first, the operator's after it, so the cursor stays at
//!    the end of the operator's text.
//! 2. **Independent regions**: both edits apply. At a shared boundary a pure
//!    insertion lands before the replacement that starts there.
//! 3. **Overlapping edits**: a true conflict. The overlapping edits of both
//!    sides are grouped (transitively) and widened to whole lines, and the
//!    region is written through [`crate::conflict_render::render_conflict`]:
//!    inline CriticMarkup for short changes, a minimal block otherwise. Nothing
//!    is dropped: resolving the region to either side reproduces that side.
//!    Independent edits that share a line with a conflict are shown inside it.
//!
//! The operator's cursor is carried through as an offset into `yours` and
//! mapped into the result; it is `None` when it sits inside a conflict.

use std::ops::Range;
use std::time::{Duration, Instant};

use similar::{Algorithm, DiffOp, capture_diff_deadline};

use crate::conflict_render::{ConflictRenderError, render_conflict};

/// Bound on the character diff of one side against the base. Past it `similar`
/// returns a coarser (still exact) edit script, which can only widen conflicts.
const DIFF_DEADLINE: Duration = Duration::from_millis(500);

/// The reconciled document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reconciled {
    pub text: String,
    /// The operator's cursor mapped into `text` (byte offset); `None` when it
    /// fell inside a surfaced conflict or no cursor was given.
    pub cursor: Option<usize>,
    /// Number of conflict regions surfaced in `text`.
    pub conflicts: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconcileError {
    /// A conflicting region already carries conflict marker syntax, so it could
    /// not be surfaced losslessly. The caller must keep the operator's buffer
    /// and retry rather than drop either side.
    MarkerSyntaxInInput,
    /// The cursor offset is not a char boundary of `yours`.
    CursorOutOfBounds,
}

impl From<ConflictRenderError> for ReconcileError {
    fn from(error: ConflictRenderError) -> Self {
        match error {
            ConflictRenderError::MarkerSyntaxInInput => Self::MarkerSyntaxInInput,
        }
    }
}

/// One side's change to the base: replace `base[range]` with `ins`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Edit {
    range: Range<usize>,
    ins: String,
}

impl Edit {
    fn is_insert(&self) -> bool {
        self.range.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Yours,
    Agent,
}

/// Rule 1 and rule 2: two edits are independent unless their spans overlap.
/// Two pure insertions at one offset are independent (rule 1).
fn overlaps(a: &Edit, b: &Edit) -> bool {
    if a.is_insert() && b.is_insert() && a.range.start == b.range.start {
        return false;
    }
    !(b.range.end <= a.range.start || a.range.end <= b.range.start)
}

/// The edits that turn `base` into `side`, grouped so each edit is one maximal
/// run of non-equal diff ops. Offsets are bytes into `base`.
fn edits(base: &str, side: &str) -> Vec<Edit> {
    let b: Vec<&str> = split_chars(base);
    let s: Vec<&str> = split_chars(side);
    let ops = capture_diff_deadline(
        Algorithm::Myers,
        &b,
        0..b.len(),
        &s,
        0..s.len(),
        Some(Instant::now() + DIFF_DEADLINE),
    );
    let b_off = byte_offsets(&b);
    let s_off = byte_offsets(&s);
    let mut out: Vec<Edit> = Vec::new();
    let mut pending: Option<(usize, usize, usize, usize)> = None;
    let flush = |pending: &mut Option<(usize, usize, usize, usize)>, out: &mut Vec<Edit>| {
        if let Some((bs, be, ss, se)) = pending.take() {
            out.push(Edit {
                range: b_off[bs]..b_off[be],
                ins: side[s_off[ss]..s_off[se]].to_string(),
            });
        }
    };
    for op in ops {
        if let DiffOp::Equal { .. } = op {
            flush(&mut pending, &mut out);
            continue;
        }
        let (_, old, new) = op.as_tag_tuple();
        pending = Some(match pending {
            Some((bs, _, ss, _)) => (bs, old.end, ss, new.end),
            None => (old.start, old.end, new.start, new.end),
        });
    }
    flush(&mut pending, &mut out);
    out
}

fn split_chars(text: &str) -> Vec<&str> {
    text.char_indices()
        .map(|(i, c)| &text[i..i + c.len_utf8()])
        .collect()
}

/// `offsets[i]` is the byte offset of token `i`; `offsets[len]` is the total.
fn byte_offsets(tokens: &[&str]) -> Vec<usize> {
    let mut offsets = Vec::with_capacity(tokens.len() + 1);
    let mut at = 0;
    offsets.push(0);
    for token in tokens {
        at += token.len();
        offsets.push(at);
    }
    offsets
}

/// A group of mutually overlapping edits from both sides, over `region`.
struct Cluster {
    region: Range<usize>,
    members: Vec<(Side, usize)>,
}

fn line_start(base: &str, at: usize) -> usize {
    base[..at].rfind('\n').map_or(0, |i| i + 1)
}

fn line_end(base: &str, at: usize) -> usize {
    if at > 0 && base.as_bytes()[at - 1] == b'\n' {
        return at;
    }
    base[at..].find('\n').map_or(base.len(), |i| at + i + 1)
}

/// Group overlapping edits (transitively) into whole-line clusters.
fn clusters(base: &str, yours: &[Edit], agent: &[Edit]) -> Vec<Cluster> {
    let all: Vec<(Side, usize, &Edit)> = yours
        .iter()
        .enumerate()
        .map(|(i, e)| (Side::Yours, i, e))
        .chain(agent.iter().enumerate().map(|(i, e)| (Side::Agent, i, e)))
        .collect();
    let mut seeds: Vec<Cluster> = Vec::new();
    for (yi, y) in yours.iter().enumerate() {
        for (gi, g) in agent.iter().enumerate() {
            if overlaps(y, g) {
                let lo = y.range.start.min(g.range.start);
                let hi = y.range.end.max(g.range.end);
                seeds.push(Cluster {
                    region: lo..hi,
                    members: vec![(Side::Yours, yi), (Side::Agent, gi)],
                });
            }
        }
    }
    // Widen each seed to whole lines, absorb every edit touching the widened
    // region, and merge seeds that meet, until nothing changes.
    seeds.sort_by_key(|c| c.region.start);
    let mut merged: Vec<Cluster> = Vec::new();
    for seed in seeds {
        let mut region = line_start(base, seed.region.start)..line_end(base, seed.region.end);
        loop {
            let mut next = region.clone();
            for (_, _, e) in &all {
                let touches = e.range.start < region.end && e.range.end > region.start
                    || (e.is_insert()
                        && e.range.start > region.start
                        && e.range.start < region.end);
                if touches {
                    next.start = next.start.min(e.range.start);
                    next.end = next.end.max(e.range.end);
                }
            }
            next = line_start(base, next.start)..line_end(base, next.end);
            if let Some(last) = merged.last()
                && last.region.end > next.start
            {
                next.start = next.start.min(last.region.start);
                next.end = next.end.max(last.region.end);
                merged.pop();
            }
            if next == region {
                break;
            }
            region = next;
        }
        merged.push(Cluster {
            region,
            members: Vec::new(),
        });
    }
    // Membership: every edit inside the region, except a pure insertion at its
    // end, which starts the next line and stays independent.
    for cluster in &mut merged {
        for (side, i, e) in &all {
            let inside = e.range.start >= cluster.region.start
                && e.range.end <= cluster.region.end
                && !(e.is_insert() && e.range.start == cluster.region.end);
            if inside {
                cluster.members.push((*side, *i));
            }
        }
    }
    merged
}

fn members<'e>(cluster: &Cluster, side: Side, list: &'e [Edit]) -> Vec<&'e Edit> {
    let mut picked: Vec<&Edit> = cluster
        .members
        .iter()
        .filter(|(s, _)| *s == side)
        .map(|(_, i)| &list[*i])
        .collect();
    picked.sort_by_key(|e| e.range.start);
    picked
}

/// Apply `edits` (sorted, disjoint) to `base[region]`.
fn apply_in(base: &str, region: &Range<usize>, edits: &[&Edit]) -> String {
    let mut out = String::new();
    let mut at = region.start;
    for e in edits {
        out.push_str(&base[at..e.range.start]);
        out.push_str(&e.ins);
        at = e.range.end;
    }
    out.push_str(&base[at..region.end]);
    out
}

/// One piece of the output, with where it came from in `yours` (for cursor
/// mapping). `exact` pieces are byte-for-byte copies of `yours[from]`.
struct Piece {
    out: Range<usize>,
    from: Option<Range<usize>>,
    exact: bool,
    conflict: bool,
    /// The operator's own edit (not unchanged base text).
    operator_edit: bool,
}

/// Reconcile `yours` (the operator's buffer) and `agent` (the agent's version),
/// both derived from `base`. `yours_cursor` is a byte offset into `yours`.
pub fn reconcile(
    base: &str,
    yours: &str,
    agent: &str,
    yours_cursor: Option<usize>,
) -> Result<Reconciled, ReconcileError> {
    if let Some(c) = yours_cursor
        && !yours.is_char_boundary(c)
    {
        return Err(ReconcileError::CursorOutOfBounds);
    }
    let y_edits = edits(base, yours);
    // Identical edits apply once: an editor that already holds the agent's
    // text (a replayed delivery) must not receive it twice.
    let g_edits: Vec<Edit> = edits(base, agent)
        .into_iter()
        .filter(|g| !y_edits.contains(g))
        .collect();
    let clusters = clusters(base, &y_edits, &g_edits);

    let in_cluster = |side: Side, i: usize| clusters.iter().any(|c| c.members.contains(&(side, i)));

    // Events in base order: independent edits and whole clusters.
    enum Event<'a> {
        Edit(Side, &'a Edit),
        Cluster(&'a Cluster),
    }
    let mut events: Vec<(usize, u8, Event)> = Vec::new();
    for (i, e) in y_edits.iter().enumerate() {
        if !in_cluster(Side::Yours, i) {
            // Rule 1: at one offset, agent insert (0) < operator insert (1) <
            // any replacement (2).
            let rank = if e.is_insert() { 1 } else { 2 };
            events.push((e.range.start, rank, Event::Edit(Side::Yours, e)));
        }
    }
    for (i, e) in g_edits.iter().enumerate() {
        if !in_cluster(Side::Agent, i) {
            let rank = if e.is_insert() { 0 } else { 2 };
            events.push((e.range.start, rank, Event::Edit(Side::Agent, e)));
        }
    }
    for c in &clusters {
        events.push((c.region.start, 3, Event::Cluster(c)));
    }
    events.sort_by_key(|(at, rank, _)| (*at, *rank));

    let mut text = String::with_capacity(base.len().max(yours.len()));
    let mut pieces: Vec<Piece> = Vec::new();
    let mut at = 0usize;
    // Offset in `yours` of the text emitted so far (events cover the base in order).
    let mut ypos = 0usize;
    let mut conflicts = 0usize;
    let mut push = |text: &mut String,
                    s: &str,
                    from: Option<Range<usize>>,
                    exact: bool,
                    conflict: bool,
                    operator_edit: bool| {
        let start = text.len();
        text.push_str(s);
        pieces.push(Piece {
            out: start..text.len(),
            from,
            exact,
            conflict,
            operator_edit,
        });
    };
    for (start, _, event) in &events {
        let start = *start;
        if start > at {
            let len = start - at;
            push(
                &mut text,
                &base[at..start],
                Some(ypos..ypos + len),
                true,
                false,
                false,
            );
            ypos += len;
        }
        match event {
            Event::Edit(Side::Yours, e) => {
                push(
                    &mut text,
                    &e.ins,
                    Some(ypos..ypos + e.ins.len()),
                    true,
                    false,
                    true,
                );
                ypos += e.ins.len();
                at = e.range.end;
            }
            Event::Edit(Side::Agent, e) => {
                // The operator still holds the base text the agent replaced.
                push(
                    &mut text,
                    &e.ins,
                    Some(ypos..ypos + e.range.len()),
                    false,
                    false,
                    false,
                );
                ypos += e.range.len();
                at = e.range.end;
            }
            Event::Cluster(c) => {
                let ys = members(c, Side::Yours, &y_edits);
                let gs = members(c, Side::Agent, &g_edits);
                let yours_side = apply_in(base, &c.region, &ys);
                let agent_side = apply_in(base, &c.region, &gs);
                let from = Some(ypos..ypos + yours_side.len());
                ypos += yours_side.len();
                if yours_side == agent_side {
                    push(&mut text, &yours_side, from, true, false, true);
                } else {
                    let rendered = render_conflict(&yours_side, &agent_side)?;
                    conflicts += 1;
                    push(&mut text, &rendered, from, false, true, false);
                }
                at = c.region.end;
            }
        }
    }
    if at < base.len() {
        push(
            &mut text,
            &base[at..],
            Some(ypos..ypos + (base.len() - at)),
            true,
            false,
            false,
        );
    }

    let cursor = yours_cursor.and_then(|c| map_cursor(&pieces, c, text.len()));
    Ok(Reconciled {
        text,
        cursor,
        conflicts,
    })
}

/// Map a `yours` offset into the output. An offset at a boundary between two
/// pieces goes to the later piece's start, so a cursor at the end of the
/// operator's insertion stays right after it (rule 1).
fn map_cursor(pieces: &[Piece], c: usize, out_len: usize) -> Option<usize> {
    // Rule 1/2: a cursor at the end of the operator's own edit stays at the end
    // of that edit, before any agent text that follows it.
    if let Some(p) = pieces.iter().find(|p| {
        p.operator_edit && p.exact && p.from.as_ref().is_some_and(|f| !f.is_empty() && f.end == c)
    }) {
        return Some(p.out.end);
    }
    for p in pieces {
        let Some(from) = &p.from else { continue };
        let contains = from.start <= c && c < from.end;
        let at_start_of_empty = from.is_empty() && from.start == c;
        if !(contains || at_start_of_empty) {
            continue;
        }
        if p.conflict {
            return None;
        }
        if at_start_of_empty && !contains {
            // An operator deletion ends exactly here, so the cursor does too.
            // A zero-width agent insertion is not where the cursor lives; keep
            // looking for the operator's own text.
            if p.exact {
                return Some(p.out.start);
            }
            continue;
        }
        return Some(if p.exact {
            p.out.start + (c - from.start)
        } else {
            p.out.start
        });
    }
    // Past every piece: the end of the document (or after a trailing conflict).
    match pieces.last() {
        Some(p) if p.conflict && p.from.as_ref().is_some_and(|f| f.end == c) => None,
        _ => Some(out_len),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conflict_render::{Keep, has_conflicts, resolve_conflicts};

    fn rec(base: &str, yours: &str, agent: &str, cursor: usize) -> Reconciled {
        reconcile(base, yours, agent, Some(cursor)).expect("reconcile")
    }

    #[test]
    fn rule1_same_point_appends_put_agent_first_and_keep_cursor() {
        let base = "intro\n";
        let yours = "intro\nmy note";
        let agent = "intro\nagent reply\n";
        let out = rec(base, yours, agent, yours.len());
        assert_eq!(out.text, "intro\nagent reply\nmy note");
        assert_eq!(out.cursor, Some(out.text.len()));
        assert_eq!(out.conflicts, 0);
    }

    #[test]
    fn rule2_operator_edit_and_agent_append_both_apply() {
        let base = "alpha beta\n\ngamma\n";
        let yours = "alpha BETA\n\ngamma\n";
        let agent = "alpha beta\n\ngamma\nagent tail\n";
        let out = rec(base, yours, agent, "alpha BETA".len());
        assert_eq!(out.text, "alpha BETA\n\ngamma\nagent tail\n");
        assert_eq!(out.cursor, Some("alpha BETA".len()));
        assert!(!has_conflicts(&out.text));
    }

    #[test]
    fn rule2_agent_edit_before_cursor_shifts_it() {
        let base = "one\ntwo\n";
        let yours = "one\ntwo!\n";
        let agent = "ONE ONE\ntwo\n";
        let out = rec(base, yours, agent, "one\ntwo!".len());
        assert_eq!(out.text, "ONE ONE\ntwo!\n");
        assert_eq!(out.cursor, Some("ONE ONE\ntwo!".len()));
    }

    #[test]
    fn rule3_same_word_conflict_is_inline_and_resolves_both_ways() {
        let base = "pending a lazy projection\nnext\n";
        let yours = "pending a quick projection\nnext\n";
        let agent = "pending a canonical projection\nnext\n";
        let out = rec(base, yours, agent, 0);
        assert_eq!(out.conflicts, 1);
        assert_eq!(
            out.text,
            "pending a {~~quick~>canonical~~} projection\nnext\n"
        );
        assert_eq!(resolve_conflicts(&out.text, Keep::Yours).unwrap(), yours);
        assert_eq!(resolve_conflicts(&out.text, Keep::Agent).unwrap(), agent);
    }

    #[test]
    fn rule3_cursor_inside_conflict_is_none() {
        let base = "a lazy b\n";
        let yours = "a quick b\n";
        let agent = "a slow b\n";
        assert_eq!(rec(base, yours, agent, 4).cursor, None);
    }

    #[test]
    fn identical_edits_are_not_a_conflict() {
        let base = "x\n";
        let out = rec(base, "y\n", "y\n", 0);
        assert_eq!(out.text, "y\n");
        assert_eq!(out.conflicts, 0);
    }

    #[test]
    fn marker_syntax_in_a_conflict_is_refused_not_dropped() {
        let base = "a b\n";
        assert_eq!(
            reconcile(base, "a {++x++}\n", "a c\n", None),
            Err(ReconcileError::MarkerSyntaxInInput)
        );
    }

    #[test]
    fn multibyte_text_keeps_char_boundaries() {
        // Both append after "café": rule 1 puts the agent's "s" first.
        let base = "café\n";
        let yours = "café au lait\n";
        let agent = "cafés\n";
        let out = rec(base, yours, agent, "café au lait".len());
        assert_eq!(out.text, "cafés au lait\n");
        assert_eq!(out.cursor, Some("cafés au lait".len()));
    }

    #[test]
    fn identical_same_point_appends_apply_once() {
        let out = rec("", "\nx\n", "\nx\n", 3);
        assert_eq!(out.text, "\nx\n");
        assert_eq!(out.cursor, Some(3));
    }

    // ---- differential check against the Lean model -------------------------

    /// Direct port of `ConflictReconciliation.merge` (Lean) over chars.
    /// Returns (yours-resolution, agent-resolution, conflict?, cursor).
    fn lean_merge(
        b: &[char],
        o: (usize, usize, &[char]),
        g: (usize, usize, &[char]),
    ) -> (String, String, bool, Option<usize>) {
        let s = |v: &[char]| v.iter().collect::<String>();
        let (op, ol, oi) = o;
        let (gp, gl, gi) = g;
        let (ostop, gstop) = (op + ol, gp + gl);
        if (op, ol, oi) == (gp, gl, gi) {
            let t = format!("{}{}{}", s(&b[..op]), s(oi), s(&b[ostop..]));
            return (t.clone(), t, false, Some(op + oi.len()));
        }
        if ol == 0 && gl == 0 && op == gp {
            let t = format!("{}{}{}{}", s(&b[..op]), s(gi), s(oi), s(&b[op..]));
            return (t.clone(), t, false, Some(op + gi.len() + oi.len()));
        }
        if gstop <= op {
            let t = format!(
                "{}{}{}{}{}",
                s(&b[..gp]),
                s(gi),
                s(&b[gstop..op]),
                s(oi),
                s(&b[ostop..])
            );
            let c = gp + gi.len() + (op - gstop) + oi.len();
            return (t.clone(), t, false, Some(c));
        }
        if ostop <= gp {
            let t = format!(
                "{}{}{}{}{}",
                s(&b[..op]),
                s(oi),
                s(&b[ostop..gp]),
                s(gi),
                s(&b[gstop..])
            );
            return (t.clone(), t, false, Some(op + oi.len()));
        }
        let apply =
            |p: usize, l: usize, i: &[char]| format!("{}{}{}", s(&b[..p]), s(i), s(&b[p + l..]));
        let (y, a) = (apply(op, ol, oi), apply(gp, gl, gi));
        let differ = y != a;
        (y, a, differ, None)
    }

    /// Every pair of single edits over a base of distinct chars with fresh
    /// inserted chars (so the diff recovers the edits exactly) matches the
    /// Lean merge: same text outside a conflict, same resolutions inside one,
    /// the same conflict verdict, and the same cursor.
    #[test]
    fn matches_the_lean_model_on_every_small_edit_pair() {
        // All-distinct chars, so a diff recovers each edit exactly.
        let base: Vec<char> = "abc\nde".chars().collect();
        let ins_o: [&[char]; 3] = [&[], &['X'], &['X', 'Y']];
        let ins_g: [&[char]; 3] = [&[], &['P'], &['P', 'Q']];
        let n = base.len();
        let mut checked = 0;
        for op in 0..=n {
            for ol in 0..=(n - op) {
                for oi in ins_o {
                    for gp in 0..=n {
                        for gl in 0..=(n - gp) {
                            for gi in ins_g {
                                // A diff never yields an edit that changes nothing; the
                                // Lean model admits one (a zero-width empty insert inside
                                // the other side's span counts as overlapping), so skip it.
                                if (ol == 0 && oi.is_empty()) || (gl == 0 && gi.is_empty()) {
                                    continue;
                                }
                                let s = |v: &[char]| v.iter().collect::<String>();
                                let base_s = s(&base);
                                let yours =
                                    format!("{}{}{}", s(&base[..op]), s(oi), s(&base[op + ol..]));
                                let agent =
                                    format!("{}{}{}", s(&base[..gp]), s(gi), s(&base[gp + gl..]));
                                let (ly, la, lconf, lcur) =
                                    lean_merge(&base, (op, ol, oi), (gp, gl, gi));
                                let ycur = s(&base[..op]).len() + s(oi).len();
                                let out = reconcile(&base_s, &yours, &agent, Some(ycur))
                                    .unwrap_or_else(|e| panic!("{e:?} for {yours:?} / {agent:?}"));
                                let ctx = format!(
                                    "o=({op},{ol},{oi:?}) g=({gp},{gl},{gi:?}) out={:?}",
                                    out.text
                                );
                                let ry = resolve_conflicts(&out.text, Keep::Yours).unwrap();
                                let ra = resolve_conflicts(&out.text, Keep::Agent).unwrap();
                                if lconf {
                                    assert_eq!(out.conflicts, 1, "{ctx}");
                                    assert_eq!(ry, ly, "{ctx}");
                                    assert_eq!(ra, la, "{ctx}");
                                } else if ly == la && lcur.is_some() {
                                    assert_eq!(out.conflicts, 0, "{ctx}");
                                    assert_eq!(out.text, ly, "{ctx}");
                                    let lcur_bytes = lcur.map(|c| {
                                        ly.chars().take(c).map(char::len_utf8).sum::<usize>()
                                    });
                                    assert_eq!(out.cursor, lcur_bytes, "{ctx}");
                                } else {
                                    // Identical overlapping edits: no conflict.
                                    assert_eq!(out.conflicts, 0, "{ctx}");
                                    assert_eq!(out.text, ly, "{ctx}");
                                }
                                checked += 1;
                            }
                        }
                    }
                }
            }
        }
        assert!(checked > 1500, "checked {checked}");
    }

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
        fn doc(&mut self, max_words: usize) -> String {
            const WORDS: [&str; 7] = ["a", "bb", " ", "\n", "cc dd", "é", "x\n"];
            (0..self.below(max_words))
                .map(|_| WORDS[self.below(WORDS.len())])
                .collect()
        }
        fn mutate(&mut self, base: &str) -> String {
            let mut chars: Vec<char> = base.chars().collect();
            for _ in 0..self.below(3) {
                let at = self.below(chars.len() + 1);
                let del = self.below(3).min(chars.len() - at);
                let ins: Vec<char> = self.doc(3).chars().collect();
                chars.splice(at..at + del, ins);
            }
            chars.into_iter().collect()
        }
    }

    /// Random multi-edit documents: nothing is lost on either side.
    #[test]
    fn generated_multi_edit_documents_lose_nothing() {
        let mut rng = Rng(0x2545_f491_4f6c_dd1d);
        for _ in 0..3000 {
            let base = rng.doc(10);
            let yours = rng.mutate(&base);
            let agent = rng.mutate(&base);
            let out = reconcile(&base, &yours, &agent, Some(yours.len())).unwrap();
            let ctx = format!(
                "base={base:?} yours={yours:?} agent={agent:?} out={:?}",
                out.text
            );
            assert_eq!(has_conflicts(&out.text), out.conflicts > 0, "{ctx}");
            let ry = resolve_conflicts(&out.text, Keep::Yours).unwrap();
            let ra = resolve_conflicts(&out.text, Keep::Agent).unwrap();
            if out.conflicts == 0 {
                assert_eq!(ry, ra, "{ctx}");
            }
            if agent == base {
                assert_eq!(out.text, yours, "{ctx}");
                assert_eq!(out.cursor, Some(out.text.len()), "{ctx}");
            }
            if yours == base {
                assert_eq!(out.text, agent, "{ctx}");
            }
            if yours == agent {
                assert_eq!(out.text, yours, "{ctx}");
            }
            if let Some(c) = out.cursor {
                assert!(out.text.is_char_boundary(c), "{ctx}");
            }
        }
    }
}
