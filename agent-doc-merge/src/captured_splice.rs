//! Rebase a captured editor splice stream over independent canonical changes.
//! This pure command planner never chooses a winner for overlapping edits.

use std::ops::Range;

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use similar::{Algorithm, DiffOp, DiffTag, capture_diff_slices};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CapturedSplice {
    pub offset_code_points: usize,
    pub delete_code_points: usize,
    pub insert: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CapturedSpliceBatch {
    pub edits: Vec<CapturedSplice>,
    pub resulting_text: String,
}

/// Translate each range through equal spans only. The complete resulting cut
/// verifies that the caller supplied the actual captured stream, not a guessed
/// edit. A batch the exact translation refuses falls back to a conflict-free
/// line-granular three-way merge ([`merge_forward`], `#heldbatchmerge`); any
/// true overlap still fails before a replacement replica is registered, and no
/// answer anchors canonical text on a line the operator deleted or rewrote
/// (`#mergecapturedsplice`).
pub fn rebase(
    base: &str,
    canonical: &str,
    batch: &CapturedSpliceBatch,
) -> Result<CapturedSpliceBatch> {
    let mut old: Vec<char> = base.chars().collect();
    // Validate the entire batch even when canonical already presents its result.
    for edit in &batch.edits {
        apply(&mut old, edit)?;
    }
    ensure!(
        old.iter().collect::<String>() == batch.resulting_text,
        "captured splice cut mismatch"
    );
    if batch.resulting_text == canonical {
        return Ok(CapturedSpliceBatch {
            edits: Vec::new(),
            resulting_text: canonical.to_owned(),
        });
    }
    let captured_result = old;
    old = base.chars().collect();
    let current: Vec<char> = canonical.chars().collect();
    // Another editor observation can publish this same burst before recovery,
    // while canonical also advances elsewhere. Prove every net captured change
    // against the same base rather than requiring whole-document equality. The
    // containment helper also recognizes an insertion retained beside the same
    // stable neighbour after the controller consumed the preceding queue head.
    if canonical_contains_captured(base, canonical, batch)? {
        return Ok(CapturedSpliceBatch {
            edits: Vec::new(),
            resulting_text: canonical.to_owned(),
        });
    }
    let planned = (|| -> Result<CapturedSpliceBatch> {
        // `#struckqueuepruneattach`: a settled struck queue row is already
        // terminal. If the operator changed *only* by deleting such rows, apply
        // those cell tombstones to canonical instead of holding the replica merely
        // because canonical decorated the same retired row. The semantic helper
        // accepts no other operator edit and preserves canonical as the output
        // spine, including every live queue head.
        if let Some(merged) = crate::crdt::merge_operator_deleted_struck_queue_items(
            base,
            &batch.resulting_text,
            canonical,
        ) {
            let target: Vec<char> = merged.chars().collect();
            let edits = splices_between(&current, &target);
            let mut replayed = current.clone();
            for edit in &edits {
                apply(&mut replayed, edit)?;
            }
            ensure!(
                replayed == target,
                "struck queue deletion splices did not reproduce the semantic merge"
            );
            return Ok(CapturedSpliceBatch {
                edits,
                resulting_text: merged,
            });
        }
        let captured_changes = char_diff(&old, &captured_result);
        let canonical_changes = char_diff(&old, &current);
        // `#steerreplicachurn`: a captured change that touches a canonical change
        // is never translated splice by splice. Equal-span translation beside an
        // overlapping canonical insert is exactly how a burst canonical already
        // ingested (a lost receipt) would be inserted a second time.
        let windows = captured_windows(
            &old,
            &current,
            &canonical_changes,
            &captured_result,
            &captured_changes,
        );
        let exact = if windows.iter().any(|window| window.canonical_ops > 0) {
            merge_windows(&captured_result, &current, &canonical_changes, &windows)
        } else {
            // A splice inside the burst may touch a canonical change even when
            // the burst's net change does not; the net change still translates.
            translate_edits(&old, &current, batch).or_else(|_| {
                merge_windows(&captured_result, &current, &canonical_changes, &windows)
            })
        };
        let answer = exact.or_else(|refusal| {
            merge_forward(&old, &current, &captured_result, &windows)
                .map_err(|merge| anyhow::anyhow!("{refusal}; merge-forward refused: {merge}"))
        })?;
        // `#mergecapturedsplice`: the planners above work by code point, and a
        // line the operator deleted or rewrote can keep a few code points by
        // coincidence; a canonical change anchored between them then lands on the
        // operator's new text. Every answer is held to whole-line anchoring:
        // where the line-granular merge exists the answer must be it, and no answer
        // keeps a canonical change whose anchor line the operator deleted or
        // rewrote. Refusing is the existing overlap outcome: the plugin keeps the
        // captured batch and the operator's visible text, and the
        // controller-licensed roll-forward settles it.
        if let Some(line_merged) = line_merge(base, &batch.resulting_text, canonical) {
            ensure!(
                answer.resulting_text == line_merged,
                "the code-point plan anchors canonical text at a different line than the \
                 line-granular merge"
            );
        }
        ensure!(
            !canonical_change_lost_its_anchor(base, canonical, &batch.resulting_text),
            "canonical changed a line the operator deleted or rewrote; its anchor is gone"
        );
        Ok(answer)
    })();
    // `#lagcanonicalrollforward`: the dual proof, consulted only when every
    // planner refused. Canonical moved past `base` only by changes the captured
    // cut already carries: the controller ingested a prefix of the operator's
    // own burst (a structure-quarantined live relay whose durable ops still
    // landed, or a lost receipt) and the operator kept typing, or it added only
    // binary-owned projection (a claim marker on the row being edited, a
    // boundary or `(HEAD)` placement) that it re-projects from its own state.
    // Every planner refuses that shape ("canonical already carries captured
    // operator text", or an overlap on the edited line), and nothing else can
    // ever change the answer, so registration held the buffer forever
    // (agent-doc-bugs.md 2026-10-26 13:48-13:53; docs.md 14:56, a queue row
    // deleted and pasted into a claimed row). The captured cut is then the
    // lossless answer: the editor text is the base and no canonical operator
    // text is dropped.
    match planned {
        Ok(answer) => Ok(answer),
        Err(refusal) => {
            if cut_carries_every_canonical_operator_change(base, canonical, &batch.resulting_text) {
                Ok(CapturedSpliceBatch {
                    edits: splices_between(&current, &captured_result),
                    resulting_text: batch.resulting_text.clone(),
                })
            } else {
                Err(refusal)
            }
        }
    }
}

/// `#lagcanonicalrollforward`: whether `captured` carries every net change
/// `base -> canonical` makes, so adopting `captured` drops nothing canonical
/// changed. Both sides are diffed against `base` (line-granular, as every
/// planner here). Each canonical change must lie inside one captured change
/// over the same base: a canonical deletion counts only when the cut replaced
/// those base characters too, and canonical's inserted text inside one captured
/// change must reappear in the cut's replacement in order, with at most
/// [`LAG_ROLL_FORWARD_MAX_INSERTED_RUNS`] run of later typing inside it and
/// nothing of it removed. Base text canonical kept and the cut
/// removed is the operator's own deletion and needs no carrying. A
/// whitespace-only canonical insertion is never counted, because it cannot be
/// told apart from an independent controller line break. A canonical change
/// found anywhere else in the cut is not proof; the refusal stands.
fn cut_carries_every_canonical_change(
    base: &[char],
    canonical: &[char],
    captured: &[char],
) -> bool {
    if canonical == base {
        return false;
    }
    // Whole changed lines, never refined by char: a char refinement of two
    // different rewrites of one line fragments them at coincidental equal
    // characters, and the fragments then never line up.
    let canonical_changes = changed_line_hunks(base, canonical);
    let captured_changes = changed_line_hunks(base, captured);
    let mut carried_inserts: Vec<Vec<char>> = vec![Vec::new(); captured_changes.len()];
    for (old, new) in &canonical_changes {
        let Some(owner) = captured_changes
            .iter()
            .position(|(within, _)| within.start <= old.start && old.end <= within.end)
        else {
            return false;
        };
        carried_inserts[owner].extend_from_slice(&canonical[new.clone()]);
    }
    carried_inserts
        .iter()
        .zip(&captured_changes)
        .all(|(inserted, (_, replacement))| {
            if inserted.is_empty() {
                return true;
            }
            if inserted.iter().all(|ch| ch.is_whitespace()) {
                return false;
            }
            let replacement = &captured[replacement.clone()];
            let mut inserted_runs = 0;
            for op in capture_diff_slices(Algorithm::Myers, inserted, replacement) {
                match op.tag() {
                    DiffTag::Equal => {}
                    DiffTag::Insert => inserted_runs += 1,
                    DiffTag::Delete | DiffTag::Replace => return false,
                }
            }
            inserted_runs <= LAG_ROLL_FORWARD_MAX_INSERTED_RUNS
        })
}

/// The changed hunks of a whole-line diff, as `(base chars, new chars)`.
fn changed_line_hunks(old: &[char], new: &[char]) -> Vec<(Range<usize>, Range<usize>)> {
    let old_lines = line_ranges(old);
    let new_lines = line_ranges(new);
    let old_keys: Vec<&[char]> = old_lines.iter().map(|range| &old[range.clone()]).collect();
    let new_keys: Vec<&[char]> = new_lines.iter().map(|range| &new[range.clone()]).collect();
    capture_diff_slices(Algorithm::Myers, &old_keys, &new_keys)
        .into_iter()
        .filter(|op| op.tag() != DiffTag::Equal)
        .map(|op| {
            (
                lines_to_chars(&old_lines, op.old_range(), old.len()),
                lines_to_chars(&new_lines, op.new_range(), new.len()),
            )
        })
        .collect()
}

/// [`cut_carries_every_canonical_change`] over operator text: binary-owned
/// markers ([`without_binary_owned_markers`]) and the attempt projection a
/// queue row carries (`🟡 [#subagent:x#1] `, lifecycle and pin markers,
/// [`agent_doc_element_queue::strip_attempt_projection_prefix`]) are removed
/// from every line first. A canonical that changed only that projection is
/// carried by any cut: the controller re-projects it from its own claim and
/// boundary state (`ProjectionDrift` → `ProjectExpectedMarker`).
fn cut_carries_every_canonical_operator_change(
    base: &str,
    canonical: &str,
    captured: &str,
) -> bool {
    if canonical == base {
        return false;
    }
    let base = operator_projection_text(base);
    let canonical = operator_projection_text(canonical);
    let captured = operator_projection_text(captured);
    let base: Vec<char> = base.chars().collect();
    let canonical: Vec<char> = canonical.chars().collect();
    let captured: Vec<char> = captured.chars().collect();
    canonical == base || cut_carries_every_canonical_change(&base, &canonical, &captured)
}

/// `text` without binary-owned markers and without the attempt projection
/// prefix of each list row.
fn operator_projection_text(text: &str) -> String {
    let unmarked = without_binary_owned_markers(text);
    let mut out = String::with_capacity(unmarked.len());
    for line in unmarked.split_inclusive('\n') {
        let indent = line.len() - line.trim_start().len();
        let rest = &line[indent..];
        match rest.strip_prefix("- ").or_else(|| rest.strip_prefix("* ")) {
            Some(body) => {
                out.push_str(&line[..indent + 2]);
                out.push_str(agent_doc_element_queue::strip_attempt_projection_prefix(
                    body,
                ));
            }
            None => out.push_str(line),
        }
    }
    out
}

/// Later typing continues at one cursor inside the region canonical already
/// carries. A second run is as likely a coincidental alignment of unrelated
/// text (`ok` inside `I looked into it`) as a continuation, so it refuses.
const LAG_ROLL_FORWARD_MAX_INSERTED_RUNS: usize = 1;

/// `#jetbrainscleanregistrati`: whether `merged`, a conflict-free three-way
/// merge of `yours` and `agent` over `base` produced by some other planner
/// (the char-level [`crate::conflict_reconcile::reconcile`]), keeps whole-line
/// anchoring: it is exactly the line-granular merge ([`line_merge`]), and no
/// change of either side sits on a base line the other side deleted or
/// rewrote into a different line. A char-level merge that fails this re-anchored
/// one side's change onto the other side's text by a coincidental code-point
/// alignment (`- do [#sdkexamples] (closed)`), so its zero conflict count is
/// not evidence of independence and the caller must refuse it.
pub fn clean_merge_keeps_line_anchors(base: &str, yours: &str, agent: &str, merged: &str) -> bool {
    line_merge(base, yours, agent).as_deref() == Some(merged)
        && !canonical_change_lost_its_anchor(base, agent, yours)
        && !canonical_change_lost_its_anchor(base, yours, agent)
}

/// `#heldbatchmerge` (GH #273): the exit for a captured batch the exact planner
/// refused only because an operator change sits BESIDE a canonical change.
///
/// The exact planner widens every change over its slide range and refuses any
/// window a canonical change touches, so independent edits that merely share a
/// line boundary refuse too: typing under a heading whose ` (HEAD)` suffix
/// canonical moved, or a queue line added next to the line a closeout struck.
/// The JetBrains plugin then held the batch (`await-missing-base`), every
/// re-registration deferred canonical behind it, and the visible buffer stayed
/// forked from the replica for good while every native save was refused.
///
/// The fallback is a line-granular three-way merge ([`line_merge`], base = the
/// captured base, yours = the captured cut, agent = canonical): the editor
/// text is the base and the unapplied canonical changes roll forward onto it.
/// It applies the rules of the formally checked reconcile
/// ([`crate::conflict_reconcile`]) with a whole base line as the unit of
/// anchoring (`#mergecapturedsplice`). It is accepted only when (1) no edit of
/// either side overlaps the other: no base line changed by both, so no winner
/// is chosen and no canonical change lands on a line the operator deleted or
/// rewrote; (2) canonical has not already ingested the operator's text, so a
/// lost receipt cannot insert a burst twice; and (3) the two sides did not
/// insert lines at one boundary, where foreign controller text may be the
/// transformation of a burst it already consumed. Otherwise the refusal
/// stands. The answer is the canonical-relative splices that produce the
/// merge, forwarded on a replica registered at exactly `canonical`.
fn merge_forward(
    base: &[char],
    canonical: &[char],
    captured_result: &[char],
    windows: &[CapturedWindow],
) -> Result<CapturedSpliceBatch> {
    ensure!(
        !operator_text_already_in_canonical(base, canonical, captured_result, windows),
        "canonical already carries captured operator text"
    );
    let base_text: String = base.iter().collect();
    let captured_text: String = captured_result.iter().collect();
    let canonical_text: String = canonical.iter().collect();
    // `#mergecapturedsplice`: the merge is line-granular. The char-level
    // reconcile this used before diffs each side with no line anchoring, so an
    // operator who deleted a line and typed others came back as a coincidental
    // alignment of the two (`- ~~do [#tdc]~~\n` against `- do [#sdkexamples]\n`
    // shares `- do [#` and the newline), and a canonical insertion anchored in
    // the deleted line landed on the operator's text with zero conflicts
    // reported (`- do [#sdkexamples] (closed)`).
    let merged = line_merge(&base_text, &captured_text, &canonical_text).ok_or_else(|| {
        anyhow::anyhow!(
            "operator and canonical edits overlap: both changed one line, an insertion falls \
             inside a run the other side replaced, or both inserted at one line boundary"
        )
    })?;
    let target: Vec<char> = merged.chars().collect();
    let edits = splices_between(canonical, &target);
    let mut replayed = canonical.to_vec();
    for edit in &edits {
        apply(&mut replayed, edit)?;
    }
    ensure!(
        replayed == target,
        "merge-forward splices did not reproduce the merge"
    );
    Ok(CapturedSpliceBatch {
        edits,
        resulting_text: merged,
    })
}

/// `#mergecapturedsplice`: the line-granular three-way merge [`merge_forward`]
/// answers with and every [`rebase`] answer is checked against. Each side
/// is diffed against `base` by whole lines (`\n`-terminated). A base line is
/// an indivisible anchor: answers `None` (an overlap) when both sides changed
/// or deleted the same base line, when one side inserts lines strictly inside
/// a run of base lines the other changed, or when both insert lines at the
/// same boundary. Otherwise both apply; at one boundary an insertion goes
/// before a change that starts there (the reconcile's rule 2). An identical
/// change made by both sides applies once, as in the reconcile. The result
/// never holds a line that is neither the operator's nor canonical's.
fn line_merge(base: &str, yours: &str, agent: &str) -> Option<String> {
    let base_lines: Vec<&str> = base.split_inclusive('\n').collect();
    let hunks =
        |side: &str| line_hunks(&base_lines, &side.split_inclusive('\n').collect::<Vec<_>>());
    let ours = hunks(yours);
    let theirs: Vec<_> = hunks(agent)
        .into_iter()
        .filter(|hunk| !ours.contains(hunk))
        .collect();
    for LineHunk { old: a, .. } in &ours {
        for LineHunk { old: b, .. } in &theirs {
            let overlap = if a.is_empty() && b.is_empty() {
                a.start == b.start
            } else if a.is_empty() {
                b.start < a.start && a.start < b.end
            } else if b.is_empty() {
                a.start < b.start && b.start < a.end
            } else {
                a.start < b.end && b.start < a.end
            };
            if overlap {
                return None;
            }
        }
    }
    let mut events: Vec<&LineHunk> = ours.iter().chain(&theirs).collect();
    events.sort_by_key(|hunk| (hunk.old.start, !hunk.old.is_empty()));
    let mut out = String::with_capacity(base.len().max(yours.len()));
    let mut at = 0;
    for hunk in events {
        out.extend(base_lines[at..hunk.old.start].iter().copied());
        out.extend(hunk.lines.iter().map(String::as_str));
        at = hunk.old.end;
    }
    out.extend(base_lines[at..].iter().copied());
    Some(out)
}

/// One whole-line edit: replace `base_lines[old]` with `lines`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LineHunk {
    old: Range<usize>,
    lines: Vec<String>,
    /// A one-line change to a line still recognisably the same line (at least
    /// half of the longer line's words in common). Anything else replaced or
    /// deleted its base lines: a change anchored inside them lost its anchor.
    edits_line: bool,
}

/// Hunks above this many old × new lines are not paired line by line.
const LINE_PAIRING_LIMIT: usize = 1_024;

/// The whole-line edits turning `base` into `side`, each as small as an exact
/// script allows. Myers groups adjacent changed lines into one hunk, and a
/// grouped hunk reads an insertion between two of its lines as an overlap
/// (`- a (HEAD)`, a boundary marker line, `- ~~b~~` is three edits, not one).
/// A replaced run is split by pairing old lines with similar new lines in
/// order: a pair is a one-line change ([`LineHunk::edits_line`]); each run of
/// unpaired lines between pairs is one hunk that deletes or replaces its base
/// lines. Every split is still an exact script.
fn line_hunks(base_lines: &[&str], lines: &[&str]) -> Vec<LineHunk> {
    let owned = |lines: &[&str]| lines.iter().map(|line| (*line).to_owned()).collect();
    let mut out = Vec::new();
    for op in capture_diff_slices(Algorithm::Myers, base_lines, lines) {
        let (old, new) = (op.old_range(), op.new_range());
        match op.tag() {
            DiffTag::Equal => {}
            DiffTag::Replace => out.extend(split_replaced_lines(
                &base_lines[old.clone()],
                &lines[new],
                old.start,
            )),
            _ => out.push(LineHunk {
                old,
                lines: owned(&lines[new]),
                edits_line: false,
            }),
        }
    }
    out
}

/// Whether `b` is still recognisably the line `a`: at least half of the longer
/// line's whitespace-separated words survive in order.
fn same_line(a: &str, b: &str) -> bool {
    let (a, b): (Vec<&str>, Vec<&str>) = (
        a.split_whitespace().collect(),
        b.split_whitespace().collect(),
    );
    let common: usize = capture_diff_slices(Algorithm::Myers, &a, &b)
        .iter()
        .filter(|op| op.tag() == DiffTag::Equal)
        .map(|op| op.old_range().len())
        .sum();
    2 * common >= a.len().max(b.len())
}

fn split_replaced_lines(old: &[&str], new: &[&str], at: usize) -> Vec<LineHunk> {
    let owned = |lines: &[&str]| lines.iter().map(|line| (*line).to_owned()).collect();
    let (n, m) = (old.len(), new.len());
    if n * m > LINE_PAIRING_LIMIT {
        return vec![LineHunk {
            old: at..at + n,
            lines: owned(new),
            edits_line: false,
        }];
    }
    // Longest in-order pairing of similar lines (LCS over `same_line`).
    let pairs: Vec<Vec<bool>> = old
        .iter()
        .map(|a| new.iter().map(|b| same_line(a, b)).collect())
        .collect();
    let mut best = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            best[i][j] = best[i + 1][j].max(best[i][j + 1]);
            if pairs[i][j] {
                best[i][j] = best[i][j].max(1 + best[i + 1][j + 1]);
            }
        }
    }
    let mut out = Vec::new();
    let (mut i, mut j, mut gap_i, mut gap_j) = (0, 0, 0, 0);
    let flush = |out: &mut Vec<LineHunk>, gap_i: usize, i: usize, gap_j: usize, j: usize| {
        if gap_i < i || gap_j < j {
            out.push(LineHunk {
                old: at + gap_i..at + i,
                lines: owned(&new[gap_j..j]),
                edits_line: false,
            });
        }
    };
    while i < n || j < m {
        if i < n && j < m && pairs[i][j] && best[i][j] == 1 + best[i + 1][j + 1] {
            flush(&mut out, gap_i, i, gap_j, j);
            if old[i] != new[j] {
                out.push(LineHunk {
                    old: at + i..at + i + 1,
                    lines: vec![new[j].to_owned()],
                    edits_line: true,
                });
            }
            i += 1;
            j += 1;
            (gap_i, gap_j) = (i, j);
        } else if i < n && (j == m || best[i][j] == best[i + 1][j]) {
            i += 1;
        } else {
            j += 1;
        }
    }
    flush(&mut out, gap_i, i, gap_j, j);
    out
}

/// `#mergecapturedsplice`: whether an answer would keep a canonical change
/// whose anchor line the operator deleted or rewrote into a different line.
///
/// The planners diff by code point, so a line the operator replaced can keep a
/// few code points by coincidence (`- do [#item4]` and `- review [#op7x4]`
/// share `- `, `[#`, `4]` and the newline). A canonical insertion between two
/// such survivors then translates onto the operator's new line
/// (`- review [#op7x4] (closed)`), the corruption the exact planner produced
/// as readily as the merge-forward fallback. Per base line: a canonical change
/// to a line the operator removed or replaced by a different line (not a
/// [`LineHunk::edits_line`] pair) has no anchor in the operator's text, and a
/// canonical line insertion is anchored only while a neighbouring base line
/// (or the document edge) survives. A change both sides made identically is not a conflict.
fn canonical_change_lost_its_anchor(base: &str, canonical: &str, captured: &str) -> bool {
    let base_lines: Vec<&str> = base.split_inclusive('\n').collect();
    let ours = line_hunks(
        &base_lines,
        &captured.split_inclusive('\n').collect::<Vec<_>>(),
    );
    let mut anchor_lost = vec![false; base_lines.len()];
    for hunk in ours.iter().filter(|hunk| !hunk.edits_line) {
        anchor_lost[hunk.old.clone()]
            .iter_mut()
            .for_each(|lost| *lost = true);
    }
    line_hunks(
        &base_lines,
        &canonical.split_inclusive('\n').collect::<Vec<_>>(),
    )
    .iter()
    .filter(|hunk| !ours.contains(hunk))
    .any(|hunk| {
        if hunk.old.is_empty() {
            let before = hunk.old.start.checked_sub(1).map(|line| anchor_lost[line]);
            let after = anchor_lost.get(hunk.old.start).copied();
            // A document edge is an anchor no edit removes.
            before.unwrap_or(false) && after.unwrap_or(false)
        } else {
            anchor_lost[hunk.old.clone()].iter().any(|lost| *lost)
        }
    })
}

/// `#heldbatchmerge` lost-receipt guard. A burst the controller already
/// ingested (whole or in part) shows up in canonical as text the base lacks.
/// Merging it again would place the operator's text twice, so the fallback
/// refuses when canonical gained an occurrence of a window's operator text, or
/// when a canonical change in the same window begins or ends with the start of
/// that text (a partially ingested prefix). Whitespace-only overlap is not
/// evidence. A false positive only keeps the pre-existing hold.
fn operator_text_already_in_canonical(
    base: &[char],
    canonical: &[char],
    captured_result: &[char],
    windows: &[CapturedWindow],
) -> bool {
    let shifted = |at: usize, shift: isize| (at as isize + shift) as usize;
    let base_text: String = base.iter().collect();
    let canonical_text: String = canonical.iter().collect();
    windows.iter().any(|window| {
        let base_window = &base[window.old.clone()];
        let ours = &captured_result[shifted(window.old.start, window.captured_shift_before)
            ..shifted(
                window.old.end,
                window.captured_shift_before + window.captured_shift_within,
            )];
        let operator: Vec<char> = trimmed_splice(0, base_window, ours)
            .insert
            .chars()
            .collect();
        let operator_text: String = operator.iter().collect();
        let needle = operator_text.trim();
        if needle.is_empty() {
            return false;
        }
        if canonical_text.matches(needle).count() > base_text.matches(needle).count() {
            return true;
        }
        if window.canonical_ops == 0 {
            return false;
        }
        let theirs = &canonical[shifted(window.old.start, window.canonical_shift_before)
            ..shifted(
                window.old.end,
                window.canonical_shift_before + window.canonical_shift_within,
            )];
        let agent: Vec<char> = trimmed_splice(0, base_window, theirs)
            .insert
            .chars()
            .collect();
        let shared = (1..=operator.len().min(agent.len()))
            .rev()
            .find(|&len| agent.starts_with(&operator[..len]) || agent.ends_with(&operator[..len]))
            .unwrap_or(0);
        operator[..shared].iter().any(|ch| !ch.is_whitespace())
    })
}

/// The splices turning `from` into `to`, one per changed hunk, in order and in
/// the coordinates of the text each is applied to.
fn splices_between(from: &[char], to: &[char]) -> Vec<CapturedSplice> {
    let mut shift = 0isize;
    char_diff(from, to)
        .into_iter()
        .filter(|op| op.tag() != DiffTag::Equal)
        .map(|op| {
            let (old, new) = (op.old_range(), op.new_range());
            let splice = CapturedSplice {
                offset_code_points: (old.start as isize + shift) as usize,
                delete_code_points: old.len(),
                insert: to[new.clone()].iter().collect(),
            };
            shift += new.len() as isize - old.len() as isize;
            splice
        })
        .collect()
}

/// The original splice-by-splice translation through equal spans.
fn translate_edits(
    base: &[char],
    canonical: &[char],
    batch: &CapturedSpliceBatch,
) -> Result<CapturedSpliceBatch> {
    let mut old = base.to_vec();
    let mut current = canonical.to_vec();
    let mut rebased = Vec::with_capacity(batch.edits.len());
    for edit in &batch.edits {
        let start = edit.offset_code_points;
        let end = start + edit.delete_code_points;
        let ops = char_diff(&old, &current);
        let mut mapped = None;
        for op in &ops {
            if op.tag() != DiffTag::Equal {
                continue;
            }
            let range = op.old_range();
            if range.start <= start && end <= range.end {
                // An insertion at a changed boundary is ambiguous. Interior
                // positions, or a true document boundary, have an exact anchor.
                if start == end
                    && ((start == range.start && start != 0)
                        || (end == range.end && end != old.len()))
                {
                    continue;
                }
                mapped = Some(op.new_range().start + start - range.start);
                break;
            }
        }
        let offset = if old == current {
            start
        } else {
            mapped.ok_or_else(|| anyhow::anyhow!("captured splice overlaps canonical changes"))?
        };
        let translated = CapturedSplice {
            offset_code_points: offset,
            delete_code_points: edit.delete_code_points,
            insert: edit.insert.clone(),
        };
        apply(&mut current, &translated)?;
        rebased.push(translated);
        apply(&mut old, edit)?;
    }
    Ok(CapturedSpliceBatch {
        edits: rebased,
        resulting_text: current.iter().collect(),
    })
}

/// A changed line hunk is refined char-by-char only below this many code points.
const HUNK_REFINE_LIMIT: usize = 16_384;

/// `#steerreplicachurn`: a char diff that stays fast on a compacted document.
///
/// Char-level Myers is O((N+M)·D). A compaction deletes most of the exchange,
/// so D approaches N, and on a 173k-char session document one call ran 45-72s
/// on the editor's per-document lane (2026-10-03, sdk.md). That stall starved
/// the open-document attach and native-reload quiesce deadlines, which is what
/// turned one stuck rebase into deregister/register churn for every document.
/// Inputs are aligned by line first; only small changed hunks are refined by
/// char. Line anchoring also keeps a compaction's deletion inside its own hunk:
/// char-level Myers over the whole document matched stray characters of the
/// deleted exchange against the queue, so a queue edit appeared to overlap the
/// compaction. The result is a valid diff (equal spans are truly equal), so
/// every caller's equal-span translation and containment proof stays sound; a
/// coarse hunk can only make a proof refuse, never accept.
fn char_diff(old: &[char], new: &[char]) -> Vec<DiffOp> {
    let old_lines = line_ranges(old);
    let new_lines = line_ranges(new);
    let old_keys: Vec<&[char]> = old_lines.iter().map(|range| &old[range.clone()]).collect();
    let new_keys: Vec<&[char]> = new_lines.iter().map(|range| &new[range.clone()]).collect();
    let mut out: Vec<DiffOp> = Vec::new();
    for op in capture_diff_slices(Algorithm::Myers, &old_keys, &new_keys) {
        let old_chars = lines_to_chars(&old_lines, op.old_range(), old.len());
        let new_chars = lines_to_chars(&new_lines, op.new_range(), new.len());
        if op.tag() == DiffTag::Equal {
            push_op(
                &mut out,
                old_chars.start,
                old_chars.len(),
                new_chars.start,
                new_chars.len(),
                true,
            );
        } else if old_chars.len() + new_chars.len() <= HUNK_REFINE_LIMIT {
            for sub in capture_diff_slices(
                Algorithm::Myers,
                &old[old_chars.clone()],
                &new[new_chars.clone()],
            ) {
                let (o, n) = (sub.old_range(), sub.new_range());
                push_op(
                    &mut out,
                    old_chars.start + o.start,
                    o.len(),
                    new_chars.start + n.start,
                    n.len(),
                    sub.tag() == DiffTag::Equal,
                );
            }
        } else {
            push_op(
                &mut out,
                old_chars.start,
                old_chars.len(),
                new_chars.start,
                new_chars.len(),
                false,
            );
        }
    }
    out
}

fn line_ranges(text: &[char]) -> Vec<Range<usize>> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (index, ch) in text.iter().enumerate() {
        if *ch == '\n' {
            lines.push(start..index + 1);
            start = index + 1;
        }
    }
    if start < text.len() {
        lines.push(start..text.len());
    }
    lines
}

fn lines_to_chars(lines: &[Range<usize>], range: Range<usize>, len: usize) -> Range<usize> {
    if range.is_empty() {
        let at = lines.get(range.start).map_or(len, |line| line.start);
        at..at
    } else {
        lines[range.start].start..lines[range.end - 1].end
    }
}

/// Append one op, coalescing adjacent equal spans and adjacent changes.
fn push_op(
    out: &mut Vec<DiffOp>,
    old_index: usize,
    old_len: usize,
    new_index: usize,
    new_len: usize,
    equal: bool,
) {
    if old_len == 0 && new_len == 0 {
        return;
    }
    if let Some(last) = out.last().copied() {
        let last_equal = last.tag() == DiffTag::Equal;
        if last_equal == equal {
            let (o, n) = (last.old_range(), last.new_range());
            out.pop();
            return push_op_raw(
                out,
                o.start,
                o.len() + old_len,
                n.start,
                n.len() + new_len,
                equal,
            );
        }
    }
    push_op_raw(out, old_index, old_len, new_index, new_len, equal);
}

fn push_op_raw(
    out: &mut Vec<DiffOp>,
    old_index: usize,
    old_len: usize,
    new_index: usize,
    new_len: usize,
    equal: bool,
) {
    out.push(if equal {
        DiffOp::Equal {
            old_index,
            new_index,
            len: old_len,
        }
    } else if new_len == 0 {
        DiffOp::Delete {
            old_index,
            old_len,
            new_index,
        }
    } else if old_len == 0 {
        DiffOp::Insert {
            old_index,
            new_index,
            new_len,
        }
    } else {
        DiffOp::Replace {
            old_index,
            old_len,
            new_index,
            new_len,
        }
    });
}

/// One region of the base the operator changed, widened to every canonical
/// change it touches (closed intervals: an insertion at either end touches).
#[derive(Debug)]
struct CapturedWindow {
    old: Range<usize>,
    /// Net code-point length change of canonical ops before this window.
    canonical_shift_before: isize,
    /// Net code-point length change of captured ops before this window.
    captured_shift_before: isize,
    canonical_shift_within: isize,
    captured_shift_within: isize,
    canonical_ops: usize,
    canonical_inserts_only: bool,
}

/// Every base range an insertion or deletion could equally be placed at.
///
/// A diff anchors an indel at one of several equivalent positions: inserting
/// `"- B\n"` before `"- c"` may come back as `"B\n- "` two code points later.
/// Two diffs of the same base against different texts can anchor one shared
/// insertion differently, and an overlap test on the raw ranges then misses it
/// (that miss is a duplicated prompt). Widening each op over its slide range
/// makes the test independent of the anchoring choice.
fn slide_range(base: &[char], op: &DiffOp, new: &[char]) -> Range<usize> {
    let old = op.old_range();
    let moved: &[char] = match op.tag() {
        DiffTag::Insert => &new[op.new_range()],
        DiffTag::Delete => &base[old.clone()],
        _ => return old,
    };
    let len = moved.len();
    let mut left = 0;
    while left < old.start && base[old.start - 1 - left] == moved[len - 1 - (left % len)] {
        left += 1;
    }
    let mut right = 0;
    while old.end + right < base.len() && base[old.end + right] == moved[right % len] {
        right += 1;
    }
    old.start - left..old.end + right
}

fn captured_windows(
    base: &[char],
    canonical_text: &[char],
    canonical: &[DiffOp],
    captured_text: &[char],
    captured: &[DiffOp],
) -> Vec<CapturedWindow> {
    let net = |op: &DiffOp| op.new_range().len() as isize - op.old_range().len() as isize;
    let mut intervals: Vec<(Range<usize>, bool, DiffOp)> = canonical
        .iter()
        .filter(|op| op.tag() != DiffTag::Equal)
        .map(|op| (slide_range(base, op, canonical_text), true, *op))
        .chain(
            captured
                .iter()
                .filter(|op| op.tag() != DiffTag::Equal)
                .map(|op| (slide_range(base, op, captured_text), false, *op)),
        )
        .collect();
    intervals.sort_by_key(|(range, _, _)| (range.start, range.end));
    let mut windows = Vec::new();
    let (mut canonical_shift, mut captured_shift) = (0isize, 0isize);
    let mut index = 0;
    while index < intervals.len() {
        let mut window = CapturedWindow {
            old: intervals[index].0.clone(),
            canonical_shift_before: canonical_shift,
            captured_shift_before: captured_shift,
            canonical_shift_within: 0,
            captured_shift_within: 0,
            canonical_ops: 0,
            canonical_inserts_only: true,
        };
        let mut has_captured = false;
        while index < intervals.len() && intervals[index].0.start <= window.old.end {
            let (range, is_canonical, op) = &intervals[index];
            window.old.end = window.old.end.max(range.end);
            if *is_canonical {
                window.canonical_ops += 1;
                window.canonical_shift_within += net(op);
                window.canonical_inserts_only &= op.tag() == DiffTag::Insert;
            } else {
                has_captured = true;
                window.captured_shift_within += net(op);
            }
            index += 1;
        }
        canonical_shift += window.canonical_shift_within;
        captured_shift += window.captured_shift_within;
        if has_captured {
            windows.push(window);
        }
    }
    windows
}

/// Rebase the operator's net change, one window at a time.
///
/// A window canonical never touched is translated as it stands. A window
/// canonical touched merges only when every canonical op there is a pure
/// insertion of real text that the operator's version of the window already
/// carries in order (a subsequence): canonical then holds an earlier state of
/// the operator's own typing, typically a burst whose receipt was lost after
/// the controller ingested it. The merge inserts only what canonical lacks, so
/// no canonical code point is deleted. Anything else still refuses.
fn merge_windows(
    captured_result: &[char],
    canonical: &[char],
    canonical_changes: &[DiffOp],
    windows: &[CapturedWindow],
) -> Result<CapturedSpliceBatch> {
    let shifted = |at: usize, shift: isize| (at as isize + shift) as usize;
    let mut current = canonical.to_vec();
    let mut edits = Vec::new();
    // Net length change of edits already emitted, in `current` coordinates.
    let mut emitted_shift = 0isize;
    for window in windows {
        let canonical_start = shifted(window.old.start, window.canonical_shift_before);
        let canonical_end = shifted(
            window.old.end,
            window.canonical_shift_before + window.canonical_shift_within,
        );
        let captured_start = shifted(window.old.start, window.captured_shift_before);
        let captured_end = shifted(
            window.old.end,
            window.captured_shift_before + window.captured_shift_within,
        );
        let ours = &captured_result[captured_start..captured_end];
        let theirs = &canonical[canonical_start..canonical_end];
        let at = shifted(canonical_start, emitted_shift);
        let window_edits = if window.canonical_ops == 0 {
            vec![trimmed_splice(at, theirs, ours)]
        } else {
            ensure!(
                window.canonical_inserts_only
                    && canonical_inserts_carry_text(canonical, canonical_changes, &window.old)
                    && is_subsequence(theirs, ours),
                "captured splice overlaps canonical changes"
            );
            insertion_splices(at, theirs, ours)
        };
        for edit in window_edits {
            emitted_shift +=
                edit.insert.chars().count() as isize - edit.delete_code_points as isize;
            apply(&mut current, &edit)?;
            edits.push(edit);
        }
    }
    Ok(CapturedSpliceBatch {
        edits,
        resulting_text: current.iter().collect(),
    })
}

fn canonical_inserts_carry_text(
    canonical: &[char],
    changes: &[DiffOp],
    window: &Range<usize>,
) -> bool {
    changes
        .iter()
        .filter(|op| op.tag() != DiffTag::Equal)
        .filter(|op| op.old_range().start >= window.start && op.old_range().end <= window.end)
        .all(|op| {
            canonical[op.new_range()]
                .iter()
                .any(|ch| !ch.is_whitespace())
        })
}

fn is_subsequence(needle: &[char], haystack: &[char]) -> bool {
    let mut rest = haystack.iter();
    needle
        .iter()
        .all(|ch| rest.any(|candidate| candidate == ch))
}

/// One splice turning `theirs` (at `at`) into `ours`, common prefix/suffix kept.
fn trimmed_splice(at: usize, theirs: &[char], ours: &[char]) -> CapturedSplice {
    let mut prefix = 0;
    while prefix < theirs.len() && prefix < ours.len() && theirs[prefix] == ours[prefix] {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < theirs.len() - prefix
        && suffix < ours.len() - prefix
        && theirs[theirs.len() - 1 - suffix] == ours[ours.len() - 1 - suffix]
    {
        suffix += 1;
    }
    CapturedSplice {
        offset_code_points: at + prefix,
        delete_code_points: theirs.len() - prefix - suffix,
        insert: ours[prefix..ours.len() - suffix].iter().collect(),
    }
}

/// Pure insertions turning `theirs` into `ours`, given `theirs` is a subsequence.
fn insertion_splices(at: usize, theirs: &[char], ours: &[char]) -> Vec<CapturedSplice> {
    let mut edits = Vec::new();
    let mut matched = 0;
    let mut pending = String::new();
    for ch in ours {
        if matched < theirs.len() && theirs[matched] == *ch {
            if !pending.is_empty() {
                edits.push(CapturedSplice {
                    offset_code_points: at + matched,
                    delete_code_points: 0,
                    insert: std::mem::take(&mut pending),
                });
            }
            matched += 1;
        } else {
            pending.push(*ch);
        }
    }
    if !pending.is_empty() {
        edits.push(CapturedSplice {
            offset_code_points: at + matched,
            delete_code_points: 0,
            insert: pending,
        });
    }
    // Offsets above count only `theirs` code points; add the code points each
    // earlier insertion already placed in front.
    let mut placed = 0;
    for edit in &mut edits {
        edit.offset_code_points += placed;
        placed += edit.insert.chars().count();
    }
    edits
}

/// `#ambiguousholdforever`: prove at editor registration that `canonical`
/// already carries every change the captured batch made to `base`, including
/// when the controller inserted its own text directly beside the operator's
/// (Myers then reports one merged insertion, which [`rebase`] rightly refuses to
/// treat as applied on the hot typing path). Each captured change must match a
/// canonical change over the same base range whose inserted text begins or ends
/// with the operator's inserted text. Registration-only: adopting canonical is
/// then lossless, where the alternative was refusing the endpoint indefinitely.
/// `#ambiguousholdforever2` / `#replayafterack`: [text] without the markers only
/// agent-doc writes: boundary-marker lines and the transient ` (HEAD)` heading
/// suffix. A controller disk projection and canonical place them differently
/// without anyone typing, so a containment proof must not count them as edits.
/// Registration's containment proof uses the stricter rule the JetBrains
/// plugin froze ([`crate::captured_splice_request::registration_operator_text`]):
/// it differs in which boundary ids, headings, and line terminators it accepts.
pub fn without_binary_owned_markers(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let trimmed = body.trim();
        if trimmed.starts_with("<!-- agent:boundary:") && trimmed.ends_with("-->") {
            continue;
        }
        let ending = &line[body.len()..];
        match body.strip_suffix(" (HEAD)") {
            Some(heading) if heading.trim_start().starts_with('#') => {
                out.push_str(heading);
                out.push_str(ending);
            }
            _ => out.push_str(line),
        }
    }
    out
}

/// True when `current` already carries every change `base -> target` makes,
/// ignoring binary-owned markers. A retained write whose delta is already in
/// the editor's cut is delivered; rebasing it again re-inserts the same text.
pub fn current_contains_delta(base: &str, target: &str, current: &str) -> bool {
    let (base, target, current) = (
        without_binary_owned_markers(base),
        without_binary_owned_markers(target),
        without_binary_owned_markers(current),
    );
    if target == current {
        return true;
    }
    if target == base {
        return false;
    }
    let old: Vec<char> = base.chars().collect();
    let new: Vec<char> = target.chars().collect();
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
    let batch = CapturedSpliceBatch {
        edits: vec![CapturedSplice {
            offset_code_points: prefix,
            delete_code_points: old.len() - prefix - suffix,
            insert: new[prefix..new.len() - suffix].iter().collect(),
        }],
        resulting_text: target.clone(),
    };
    canonical_contains_captured(&base, &current, &batch).unwrap_or(false)
}

pub fn canonical_contains_captured(
    base: &str,
    canonical: &str,
    batch: &CapturedSpliceBatch,
) -> Result<bool> {
    let mut captured: Vec<char> = base.chars().collect();
    for edit in &batch.edits {
        apply(&mut captured, edit)?;
    }
    ensure!(
        captured.iter().collect::<String>() == batch.resulting_text,
        "captured splice cut mismatch"
    );
    if batch.resulting_text == canonical {
        return Ok(true);
    }
    let old: Vec<char> = base.chars().collect();
    let current: Vec<char> = canonical.chars().collect();
    let captured_changes = char_diff(&old, &captured);
    let canonical_changes = char_diff(&old, &current);
    let exact_range_containment = captured_changes
        .iter()
        .filter(|op| op.tag() != DiffTag::Equal)
        .all(|change| {
            let inserted = &captured[change.new_range()];
            canonical_changes.iter().any(|observed| {
                let observed_inserted = &current[observed.new_range()];
                observed.tag() != DiffTag::Equal
                    && observed.old_range() == change.old_range()
                    && (observed_inserted.starts_with(inserted)
                        || observed_inserted.ends_with(inserted))
            })
        });
    if exact_range_containment {
        return Ok(true);
    }

    // `#apiqueuedup`: queue consumption can delete the selected head immediately
    // before an operator insertion. The insertion is then present in canonical
    // beside the same queue-close marker, but its base offset moved, so the
    // exact-range proof above misses it. A three-way line merge treated the two
    // spellings as independent and emitted the prompt twice. Prove this narrow
    // case by an unchanged neighbour: every net operator change must be a pure
    // insertion found next to the same non-whitespace base prefix or suffix.
    // This is deliberately not a document-wide substring/multiplicity heuristic;
    // repeated text at another location remains distinct operator intent.
    let non_equal: Vec<_> = captured_changes
        .iter()
        .filter(|op| op.tag() != DiffTag::Equal)
        .collect();
    Ok(!non_equal.is_empty()
        && non_equal.iter().all(|change| {
            change.old_range().is_empty()
                && anchored_insertion_is_present(&old, &captured, &current, change)
        }))
}

const CONTAINMENT_ANCHOR_CHARS: usize = 32;

fn anchored_insertion_is_present(
    base: &[char],
    captured: &[char],
    canonical: &[char],
    change: &DiffOp,
) -> bool {
    let inserted = &captured[change.new_range()];
    if inserted.is_empty() {
        return false;
    }
    let at = change.old_range().start;
    let right_end = (at + CONTAINMENT_ANCHOR_CHARS).min(base.len());
    let right = &base[at..right_end];
    if anchor_is_meaningful(right) && contains_joined(canonical, inserted, right) {
        return true;
    }
    let left_start = at.saturating_sub(CONTAINMENT_ANCHOR_CHARS);
    let left = &base[left_start..at];
    anchor_is_meaningful(left) && contains_joined(canonical, left, inserted)
}

fn anchor_is_meaningful(anchor: &[char]) -> bool {
    anchor.iter().any(|ch| !ch.is_whitespace())
}

fn contains_joined(haystack: &[char], left: &[char], right: &[char]) -> bool {
    let needle_len = left.len() + right.len();
    needle_len <= haystack.len()
        && haystack.windows(needle_len).any(|window| {
            let (window_left, window_right) = window.split_at(left.len());
            window_left == left && window_right == right
        })
}

fn apply(text: &mut Vec<char>, edit: &CapturedSplice) -> Result<()> {
    let end = edit
        .offset_code_points
        .checked_add(edit.delete_code_points)
        .ok_or_else(|| anyhow::anyhow!("captured splice range overflow"))?;
    ensure!(
        edit.offset_code_points <= text.len() && end <= text.len(),
        "captured splice outside base"
    );
    text.splice(edit.offset_code_points..end, edit.insert.chars());
    Ok(())
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

    fn batch(base: &str, edits: Vec<CapturedSplice>) -> CapturedSpliceBatch {
        let mut text = base.chars().collect();
        for edit in &edits {
            apply(&mut text, edit).unwrap();
        }
        CapturedSpliceBatch {
            edits,
            resulting_text: text.iter().collect(),
        }
    }

    /// `#ambiguousholdforever`: the registration hold's proof. The operator
    /// pasted after the shadow; the controller ingested that paste, then added
    /// a response below it and rewrote the compaction header. Canonical already
    /// holds every operator change, so the rebase must answer with no edits.
    #[test]
    fn operator_edits_already_in_advanced_canonical_are_contained() {
        let shadow = "<!-- agent:exchange -->\n*Compacted 003942*\nFix api.md issue\n<!-- /agent:exchange -->\n";
        let buffer = "<!-- agent:exchange -->\n*Compacted 003942*\nFix api.md issue\n```\n• Failed (exit 1) write --commit\n```\n<!-- /agent:exchange -->\n";
        let canonical = "<!-- agent:exchange -->\n*Compacted 004013*\nFix api.md issue\n```\n• Failed (exit 1) write --commit\n```\n\n### Re: infra.md — opus\n\nReplayed.\n<!-- /agent:exchange -->\n";
        let prefix = shadow.find("<!-- /agent:exchange").unwrap();
        let pasted = &buffer[prefix..buffer.len() - (shadow.len() - prefix)];
        let edits = batch(
            shadow,
            vec![edit(shadow[..prefix].chars().count(), 0, pasted)],
        );
        assert_eq!(edits.resulting_text, buffer);
        assert!(canonical_contains_captured(shadow, canonical, &edits).unwrap());

        // Control: canonical WITHOUT the paste does not contain it.
        let without_paste = canonical.replace("```\n• Failed (exit 1) write --commit\n```\n", "");
        assert!(!canonical_contains_captured(shadow, &without_paste, &edits).unwrap());
        // Control: a different operator edit at the same anchor is not contained.
        let other = batch(
            shadow,
            vec![edit(shadow[..prefix].chars().count(), 0, "other text\n")],
        );
        assert!(!canonical_contains_captured(shadow, canonical, &other).unwrap());
        // Control: an edit elsewhere that canonical never saw is not contained.
        let elsewhere = batch(shadow, vec![edit(0, 0, "operator header\n")]);
        assert!(!canonical_contains_captured(shadow, canonical, &elsewhere).unwrap());
    }

    #[test]
    fn translates_unicode_batch_over_independent_prefix_insertion() {
        let base = "head\nHello 🌍 world\n";
        let edits = batch(base, vec![edit(13, 0, "new "), edit(17, 5, "reader")]);
        let canonical = format!("response\n{base}");
        let out = rebase(base, &canonical, &edits).unwrap();
        assert_eq!(out.resulting_text, "response\nhead\nHello 🌍 new reader\n");
    }

    #[test]
    fn translates_delete_over_independent_suffix_change() {
        let base = "first paragraph\nsecond paragraph\n";
        let out = rebase(
            base,
            "first paragraph\nnew final paragraph\n",
            &batch(base, vec![edit(6, 4, "")]),
        )
        .unwrap();
        assert_eq!(out.resulting_text, "first graph\nnew final paragraph\n");
    }

    #[test]
    fn rejects_overlap_and_corrupt_or_out_of_range_captures() {
        let base = "hello world";
        assert!(
            rebase(
                base,
                "hello reader",
                &batch(base, vec![edit(6, 5, "friend")])
            )
            .is_err()
        );
        let mut bad = batch(base, vec![edit(1, 1, "a")]);
        bad.resulting_text = "invented".into();
        assert!(rebase(base, "invented", &bad).is_err());
        bad.edits[0].offset_code_points = usize::MAX;
        assert!(rebase(base, base, &bad).is_err());
    }

    #[test]
    fn exact_base_and_already_applied_batch() {
        let e = edit(5, 0, "!");
        let edits = batch("hello", vec![e.clone()]);
        assert_eq!(rebase("hello", "hello", &edits).unwrap().edits, vec![e]);
        assert!(rebase("hello", "hello!", &edits).unwrap().edits.is_empty());
    }

    #[test]
    fn retained_response_and_operator_queue_edit_both_survive() {
        let base = "<!-- agent:exchange -->\nresponse\n<!-- /agent:exchange -->\n<!-- agent:queue -->\n- fix bug\n<!-- /agent:queue -->\n";
        let canonical = base.replace("response\n", "response\nnew answer\n");
        let offset = base.find("bug").unwrap(); // fixture is ASCII
        let edits = batch(base, vec![edit(offset, 3, "queue retrieval")]);
        let out = rebase(base, &canonical, &edits).unwrap();
        assert_eq!(
            out.resulting_text,
            canonical.replace("fix bug", "fix queue retrieval")
        );
        assert_eq!(
            out.edits[0].offset_code_points,
            offset + "new answer\n".len()
        );
    }

    #[test]
    fn independent_changes_do_not_allow_an_insertion_inside_replaced_text() {
        assert!(
            rebase(
                "one two three",
                "one NEW three",
                &batch("one two three", vec![edit(5, 0, "x")])
            )
            .is_err()
        );
    }

    #[test]
    fn already_published_typing_burst_with_independent_response_is_not_duplicated() {
        let base = "answer\nfix bug\n";
        let edits = batch(base, vec![edit(14, 0, " n"), edit(16, 0, "ow")]);
        let canonical = edits
            .resulting_text
            .replace("answer\n", "answer\nnew response\n");
        assert!(rebase(base, &canonical, &edits).unwrap().edits.is_empty());
    }

    /// The one splice the JetBrains plugin hands the proof: common prefix and
    /// suffix kept, everything between replaced.
    fn single_splice(base: &str, after: &str) -> CapturedSpliceBatch {
        let old: Vec<char> = base.chars().collect();
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
        let insert: String = new[prefix..new.len() - suffix].iter().collect();
        batch(
            base,
            vec![edit(prefix, old.len() - prefix - suffix, &insert)],
        )
    }

    /// `#ambiguousholdforever2`: the fpe.md hold (2026-09-29). The buffer was
    /// reloaded from a controller disk projection that put ` (HEAD)` and the
    /// boundary at the response heading; canonical kept its boundary at the end
    /// of the exchange and gained merge debris after the last component. The
    /// operator's only edit, a queue line, WAS in canonical. With the markers
    /// left in, the marker change is never "contained" and the hold never ends;
    /// normalized, the operator edit is proven and canonical is adopted.
    #[test]
    fn marker_only_difference_blocks_containment_until_markers_are_normalized() {
        let shadow = "<!-- agent:exchange -->\n### Re: FPE capacity recommendation\n\nIncrease CPU first.\n<!-- agent:boundary:60c81193 -->\n<!-- /agent:exchange -->\n<!-- agent:queue -->\n- The performance seems slow.\n<!-- /agent:queue -->\n<!-- /agent:done -->\n";
        let buffer = "<!-- agent:exchange -->\n<!-- agent:boundary:60c81193 -->\n### Re: FPE capacity recommendation (HEAD)\n\nIncrease CPU first.\n<!-- /agent:exchange -->\n<!-- agent:queue -->\n- The performance seems slow.\n- PR #194 is merged. Continue.\n<!-- /agent:queue -->\n<!-- /agent:done -->\n";
        let canonical = "<!-- agent:exchange -->\n### Re: FPE capacity recommendation\n\nIncrease CPU first.\n<!-- agent:boundary:60c81193 -->\n<!-- /agent:exchange -->\n<!-- agent:queue -->\n- The performance seems slow.\n- PR #194 is merged. Continue.\n<!-- /agent:queue -->\n<!-- /agent:done -->\nIncrease CPU first.\n60c81193 -->\n";

        assert!(
            !canonical_contains_captured(shadow, canonical, &single_splice(shadow, buffer))
                .unwrap(),
            "the raw proof must reproduce the hold"
        );

        let (shadow, buffer, canonical) = (
            without_binary_owned_markers(shadow),
            without_binary_owned_markers(buffer),
            without_binary_owned_markers(canonical),
        );
        assert!(
            canonical_contains_captured(&shadow, &canonical, &single_splice(&shadow, &buffer))
                .unwrap()
        );

        // Control: normalization does not excuse an operator edit canonical lacks.
        let unseen = buffer.replace(
            "- PR #194 is merged. Continue.\n",
            "- typed while detached\n",
        );
        assert!(
            !canonical_contains_captured(&shadow, &canonical, &single_splice(&shadow, &unseen))
                .unwrap()
        );
    }

    /// `#replayafterack`: fpe.md 2026-09-29. The retained write's delta (the
    /// response, boundary at the end of the exchange) was already in the
    /// editor's cut, which the disk projection had shaped with ` (HEAD)` and the
    /// boundary above the heading, plus an operator queue line.
    #[test]
    fn a_delta_already_in_the_cut_under_different_markers_is_contained() {
        let base = "<!-- agent:exchange -->\nprompt\n<!-- agent:boundary:60c81193 -->\n<!-- /agent:exchange -->\n<!-- agent:queue -->\n- slow?\n<!-- /agent:queue -->\n";
        let target = "<!-- agent:exchange -->\nprompt\n### Re: FPE capacity recommendation\n\nIncrease CPU first.\n<!-- agent:boundary:60c81193 -->\n<!-- /agent:exchange -->\n<!-- agent:queue -->\n- slow?\n<!-- /agent:queue -->\n";
        let cut = "<!-- agent:exchange -->\nprompt\n<!-- agent:boundary:60c81193 -->\n### Re: FPE capacity recommendation (HEAD)\n\nIncrease CPU first.\n<!-- /agent:exchange -->\n<!-- agent:queue -->\n- slow?\n- PR #194 is merged.\n<!-- /agent:queue -->\n";
        assert!(current_contains_delta(base, target, cut));
        // Control: a cut without the response does not contain the delta.
        let without = cut.replace(
            "### Re: FPE capacity recommendation (HEAD)\n\nIncrease CPU first.\n",
            "",
        );
        assert!(!current_contains_delta(base, target, &without));
        // Control: a different response body under the same heading is not the delta.
        let other = cut.replace("Increase CPU first.", "Buy GPUs.");
        assert!(!current_contains_delta(base, target, &other));
    }

    /// `#steerreplicachurn` fixtures: a session document whose exchange the
    /// controller compacted while the operator typed into the queue.
    const QUEUE_HEAD: &str = "<!-- agent:queue priority go -->\n";
    const FIRST_PROMPT: &str = "- Can we extract a separate base cpp sdk from unreal?";
    const PROMPT_TAIL: &str = " and can we have the unreal sdk depend on the cpp sdk?";
    const SECOND_PROMPT: &str = "\n- Can we integrate blueprints support into the unreal sdk?";

    fn session(exchange: &str, queue_prefix: &str, backlog: &str) -> String {
        format!(
            "---\nagent: claude\n---\n\n## Exchange\n\n<!-- agent:exchange -->\n{exchange}<!-- agent:boundary:555fa5c8:sdk -->\n<!-- /agent:exchange -->\n\n# Queue\n\n{QUEUE_HEAD}{queue_prefix}- do [#sdkluau3]\n- do [#sdkjira1361]\n<!-- /agent:queue -->\n\n# Backlog\n\n<!-- agent:backlog priority queue -->\n{backlog}- [ ] [#sdkluau3] luau parity\n<!-- /agent:backlog -->\n"
        )
    }

    fn long_exchange(responses: usize) -> String {
        (0..responses)
            .map(|n| format!("### Re: topic {n}\n\nAnswer {n} with enough prose to make the exchange realistic.\n\n"))
            .collect()
    }

    /// Typing, one code point per splice, starting at `offset`.
    fn typed(offset: usize, typed: &str) -> Vec<CapturedSplice> {
        typed
            .chars()
            .enumerate()
            .map(|(index, ch)| edit(offset + index, 0, &ch.to_string()))
            .collect()
    }

    /// The sdk.md strand (2026-10-03 20:50Z). The operator typed a prompt; its
    /// push was ingested (`crdt_document_op_delta_ingested delta_len=158`) but
    /// the editor saw `durable=false`, so the burst stayed captured against the
    /// pre-compaction shadow while the operator kept typing. Canonical then held
    /// the compacted exchange, that first burst, and a new backlog item. Every
    /// retry refused with "captured splice overlaps canonical changes", so the
    /// prompt lived only in the IDE buffer and every preflight was refused.
    fn stranded_prompt_fixture(responses: usize) -> (String, CapturedSpliceBatch, String) {
        let shadow = session(&long_exchange(responses), "", "");
        let queue_at = shadow.find(QUEUE_HEAD).unwrap() + QUEUE_HEAD.len();
        let at = shadow[..queue_at].chars().count();
        let mut edits = typed(at, &format!("{FIRST_PROMPT}\n"));
        let tail_at = at + FIRST_PROMPT.chars().count();
        edits.extend(typed(tail_at, PROMPT_TAIL));
        let second_at = tail_at + PROMPT_TAIL.chars().count();
        edits.extend(typed(second_at, SECOND_PROMPT));
        let captured = batch(&shadow, edits);
        let canonical = session(
            "### Session Summary\n\n*Compacted. Content archived.*\n",
            &format!("{FIRST_PROMPT}\n"),
            "- [ ] [#sdkluau2bci] luau-analyze is missing in CI\n",
        );
        (shadow, captured, canonical)
    }

    fn expected_merge(canonical: &str) -> String {
        canonical.replace(
            &format!("{FIRST_PROMPT}\n"),
            &format!("{FIRST_PROMPT}{PROMPT_TAIL}{SECOND_PROMPT}\n"),
        )
    }

    #[test]
    fn ingested_burst_plus_later_typing_rebases_over_compaction_without_duplication() {
        let (shadow, captured, canonical) = stranded_prompt_fixture(40);
        let out = rebase(&shadow, &canonical, &captured).expect("the operator prompt must rebase");
        assert_eq!(out.resulting_text, expected_merge(&canonical));
        assert_eq!(
            out.resulting_text.matches("from unreal?").count(),
            1,
            "no duplicated burst"
        );
        // Pure insertions: no canonical code point is deleted.
        assert!(out.edits.iter().all(|edit| edit.delete_code_points == 0));
        let mut replay: Vec<char> = canonical.chars().collect();
        for edit in &out.edits {
            apply(&mut replay, edit).unwrap();
        }
        assert_eq!(replay.iter().collect::<String>(), out.resulting_text);
    }

    /// The same strand at registration: the splice stream was dropped with the
    /// retired native generation, so the plugin hands the rebase one splice
    /// from its settled shadow to the live buffer.
    #[test]
    fn registration_single_splice_rebases_the_stranded_prompt() {
        let (shadow, captured, canonical) = stranded_prompt_fixture(40);
        let out = rebase(
            &shadow,
            &canonical,
            &single_splice(&shadow, &captured.resulting_text),
        )
        .expect("registration must merge the live buffer forward");
        assert_eq!(out.resulting_text, expected_merge(&canonical));
    }

    #[test]
    fn overlapping_controller_text_or_deletion_still_refuses() {
        let (shadow, captured, canonical) = stranded_prompt_fixture(4);
        // Controller text at the operator's anchor that the operator never typed.
        let foreign = canonical.replace(FIRST_PROMPT, "- consumed by the controller");
        assert!(rebase(&shadow, &foreign, &captured).is_err());
        // A canonical deletion beside the operator's edit is not an earlier
        // state of the operator's typing; it must not be resurrected.
        let deleted = canonical.replace(
            &format!("{FIRST_PROMPT}\n- do [#sdkluau3]\n"),
            &format!("{FIRST_PROMPT}\n"),
        );
        assert!(rebase(&shadow, &deleted, &captured).is_err());
        // A whitespace-only canonical insertion is never absorbed.
        let base = "a\n";
        let ws = batch(base, vec![edit(1, 0, "\nb")]);
        assert!(rebase(base, "a\n\n", &ws).is_err());
    }

    /// The anchored diff is the char diff below the limit and a valid diff above it.
    #[test]
    fn anchored_char_diff_reconstructs_both_sides() {
        let (shadow, captured, canonical) = stranded_prompt_fixture(400);
        let old: Vec<char> = shadow.chars().collect();
        assert!(
            old.len() > HUNK_REFINE_LIMIT,
            "fixture must exceed one refined hunk"
        );
        for new in [
            captured.resulting_text.chars().collect::<Vec<_>>(),
            canonical.chars().collect(),
        ] {
            let mut rebuilt = Vec::new();
            let mut cursor = 0;
            for op in char_diff(&old, &new) {
                assert_eq!(op.old_range().start, cursor, "ops must tile the old side");
                cursor = op.old_range().end;
                if op.tag() == DiffTag::Equal {
                    assert_eq!(old[op.old_range()], new[op.new_range()]);
                }
                rebuilt.extend_from_slice(&new[op.new_range()]);
            }
            assert_eq!(cursor, old.len());
            assert_eq!(rebuilt, new);
        }
    }

    /// The 45-72s native rebase that held the document lane. Generous bound:
    /// the anchored diff answers in milliseconds, char-level Myers in minutes.
    #[test]
    fn large_compaction_rebase_stays_off_the_slow_path() {
        let (shadow, captured, canonical) = stranded_prompt_fixture(1500);
        assert!(shadow.chars().count() > 100_000);
        let started = std::time::Instant::now();
        let out = rebase(&shadow, &canonical, &captured).unwrap();
        assert_eq!(out.resulting_text, expected_merge(&canonical));
        let elapsed = started.elapsed();
        assert!(
            elapsed < std::time::Duration::from_secs(20),
            "rebase took {elapsed:?}"
        );
    }

    #[test]
    fn binary_owned_markers_are_normalized_but_operator_mentions_are_kept() {
        assert_eq!(
            without_binary_owned_markers(
                "a\n  <!-- agent:boundary:ab12 -->\n### Re: x (HEAD)\nb (HEAD)\n"
            ),
            "a\n### Re: x\nb (HEAD)\n"
        );
        assert_eq!(
            without_binary_owned_markers("t\n<!-- agent:boundary:ab12 -->"),
            "t\n"
        );
    }

    #[test]
    fn queue_prompt_moved_by_consume_is_not_replayed_beside_itself() {
        let old_prompt = "Is PR #666 merged into dev?";
        let next_prompt = "Make another PR to merge #666 into `dev`.";
        let base =
            format!("exchange\n<!-- agent:queue go -->\n{old_prompt}\n<!-- /agent:queue -->\n");
        let buffer = base.replace(old_prompt, &format!("{old_prompt}\n{next_prompt}"));
        let canonical = format!(
            "exchange\nanswer to old prompt\n<!-- agent:queue -->\n{next_prompt}\n<!-- /agent:queue -->\n"
        );
        let at = base.find("\n<!-- /agent:queue -->").unwrap();
        let captured = batch(
            &base,
            vec![edit(
                base[..at].chars().count(),
                0,
                &format!("\n{next_prompt}"),
            )],
        );

        let line_merge = crate::conflict_reconcile::reconcile(&base, &buffer, &canonical, None)
            .expect("line merge fixture should remain conflict-free");
        assert_eq!(line_merge.conflicts, 0);
        assert_eq!(line_merge.text.matches(next_prompt).count(), 2);

        assert!(canonical_contains_captured(&base, &canonical, &captured).unwrap());
        let rebased = rebase(&base, &canonical, &captured)
            .expect("capture-aware rebase should recognize the moved prompt");
        assert_eq!(rebased.resulting_text, canonical);
        assert_eq!(rebased.resulting_text.matches(next_prompt).count(), 1);

        let elsewhere = format!(
            "{next_prompt}\nexchange\nanswer to old prompt\n<!-- agent:queue -->\n<!-- /agent:queue -->\n"
        );
        assert!(
            !canonical_contains_captured(&base, &elsewhere, &captured).unwrap(),
            "the same text away from its stable queue neighbour is not containment"
        );
        assert!(
            rebase(&base, &elsewhere, &captured).is_err(),
            "an unrelated equal string cannot consume operator intent"
        );
    }

    /// GH #273 (`#heldbatchmerge`), incident shape. After a controller
    /// recycle the operator typed into a buffer whose base lacked the
    /// ` (HEAD)` suffix canonical carried (the 7-char shadow/base gap), then a
    /// closeout struck a queue line and appended its response. Each operator
    /// change sits beside, not on, a canonical change, so the exact planner's
    /// widened windows refused it; the plugin held the 609-splice batch with
    /// `await-missing-base` and the visible buffer stayed forked for good.
    #[test]
    fn gh273_batch_beside_head_marker_and_closeout_merges_forward() {
        let base = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: earlier\n",
            "\n",
            "answer\n",
            "<!-- agent:boundary:5a1b2c3d -->\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:queue go -->\n",
            "- do [#a]: first\n",
            "<!-- /agent:queue -->\n",
        );
        let under_heading = "### Re: earlier\n";
        let after_line = "- do [#a]: first\n";
        let at_heading = base[..base.find(under_heading).unwrap() + under_heading.len()]
            .chars()
            .count();
        let mut edits = typed(at_heading, "operator note under the heading\n");
        let queue_at = base[..base.find(after_line).unwrap() + after_line.len()]
            .chars()
            .count()
            + "operator note under the heading\n".chars().count();
        edits.extend(typed(queue_at, "- do [#b]: typed while detached\n"));
        let captured = batch(base, edits);
        let canonical = base
            .replace("### Re: earlier\n", "### Re: earlier (HEAD)\n")
            .replace("- do [#a]: first", "- ~~do [#a]: first~~")
            .replace(
                "answer\n",
                "answer\n\n### Re: closeout (HEAD)\n\nclosed it.\n",
            )
            .replace("### Re: earlier (HEAD)", "### Re: earlier");

        let out = rebase(base, &canonical, &captured)
            .expect("independent edits beside canonical changes must merge forward");
        assert_eq!(
            out.resulting_text,
            concat!(
                "<!-- agent:exchange -->\n",
                "### Re: earlier\n",
                "operator note under the heading\n",
                "\n",
                "answer\n",
                "\n### Re: closeout (HEAD)\n\nclosed it.\n",
                "<!-- agent:boundary:5a1b2c3d -->\n",
                "<!-- /agent:exchange -->\n",
                "<!-- agent:queue go -->\n",
                "- ~~do [#a]: first~~\n",
                "- do [#b]: typed while detached\n",
                "<!-- /agent:queue -->\n",
            )
        );
        // The splices are canonical-relative, so forwarding them on a replica
        // registered at exactly `canonical` reproduces the merge.
        let mut replay: Vec<char> = canonical.chars().collect();
        for edit in &out.edits {
            apply(&mut replay, edit).unwrap();
        }
        assert_eq!(replay.iter().collect::<String>(), out.resulting_text);
        assert!(
            out.edits.iter().all(|edit| edit.delete_code_points == 0),
            "no canonical code point is deleted"
        );

        // The ` (HEAD)` half alone: the shadow the plugin held carried the
        // suffix the operator's base lacked.
        let head_only = base.replace("### Re: earlier\n", "### Re: earlier (HEAD)\n");
        let out = rebase(base, &head_only, &captured).expect("a moved marker is not a conflict");
        assert!(
            out.resulting_text
                .contains("### Re: earlier (HEAD)\noperator note under the heading\n")
        );
        assert_eq!(
            out.resulting_text.matches("typed while detached").count(),
            1
        );
    }

    /// `#heldbatchmerge` refusals: the fallback never chooses a winner, never
    /// re-inserts text canonical already ingested, and never places operator
    /// text beside controller text written at the operator's own anchor.
    #[test]
    fn merge_forward_keeps_every_ambiguous_refusal() {
        let base = "<!-- agent:queue -->\n- do [#a]: first\n<!-- /agent:queue -->\nnotes\n";
        let line = "- do [#a]: first";
        let at = base[..base.find(line).unwrap()].chars().count();
        // Overlap: both sides rewrote the same queue line.
        let first = at + "- do [#a]: ".chars().count();
        let operator = batch(base, vec![edit(first, 5, "third")]);
        let rewritten = base.replace("[#a]: first", "[#a]: second");
        let error = rebase(base, &rewritten, &operator).unwrap_err().to_string();
        assert!(error.contains("merge-forward refused"), "{error}");
        // Lost receipt, partially ingested: canonical holds the operator's
        // first characters at the anchor plus a controller line after them.
        let notes_at = base[..base.find("notes").unwrap()].chars().count();
        let typed_burst = batch(base, typed(notes_at, "follow up on #a\n"));
        let partial = base.replace("notes\n", "follow up\n- controller line\nnotes\n");
        assert!(rebase(base, &partial, &typed_burst).is_err());
        // Lost receipt, whole burst ingested beside a canonical change in the
        // operator's window.
        let ingested = base.replace("notes\n", "follow up on #a\n- controller line\nnotes\n");
        let out = rebase(base, &ingested, &typed_burst);
        assert!(
            out.as_ref().map_or(true, |out| out
                .resulting_text
                .matches("follow up on #a")
                .count()
                == 1),
            "{out:?}"
        );
        // Same anchor: controller text exactly where the operator typed.
        let same_point = base.replace("notes\n", "controller summary\nnotes\n");
        assert!(rebase(base, &same_point, &typed_burst).is_err());
    }

    /// `#heldbatchmerge` property: for every generated document where the
    /// operator and canonical edit different lines (adjacent allowed, never the
    /// same line or the same insertion point), the planner answers a batch, the
    /// batch replayed on canonical reproduces its own cut, and that cut is the
    /// line-wise union of both sides. Deterministic xorshift, no new deps.
    #[test]
    fn independent_line_edits_always_rebase_or_merge_forward_to_their_union() {
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = move |bound: usize| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % bound as u64) as usize
        };
        for case in 0..300 {
            let lines = 4 + next(10);
            let base_lines: Vec<String> = (0..lines).map(|n| format!("- item {n} body;")).collect();
            let mut canonical_lines = Vec::new();
            let mut merged_lines = Vec::new();
            let mut operator_lines = Vec::new();
            for (n, line) in base_lines.iter().enumerate() {
                // Canonical: 0 untouched, 1 ` (HEAD)` suffix, 2 struck, 3 a
                // boundary marker line after it. Operator: 0 untouched, 1 a
                // typed line after it, 2 amended at its end. A pair that would
                // share one insertion point (amend + any canonical line edit,
                // typed line + marker) is outside the property: those refuse.
                let canonical_op = next(4);
                let operator_op = match canonical_op {
                    0 => next(3),
                    3 => 0,
                    _ => next(2),
                };
                let canonical_line = match canonical_op {
                    1 => format!("{line} (HEAD)"),
                    2 => line.replace("- item", "- ~~item").replace(';', ";~~"),
                    _ => line.clone(),
                };
                let operator_line = match operator_op {
                    2 => format!("{line} operator {case} amended {n}"),
                    _ => line.clone(),
                };
                canonical_lines.push(canonical_line.clone());
                operator_lines.push(operator_line.clone());
                merged_lines.push(if operator_op == 2 {
                    operator_line
                } else {
                    canonical_line
                });
                if operator_op == 1 {
                    let typed_line = format!("> operator {case} typed {n}");
                    operator_lines.push(typed_line.clone());
                    merged_lines.push(typed_line);
                }
                if canonical_op == 3 {
                    let marker = format!("<!-- agent:boundary:{case:08x}{n} -->");
                    canonical_lines.push(marker.clone());
                    merged_lines.push(marker);
                }
            }
            let join = |lines: &[String]| {
                lines
                    .iter()
                    .map(|line| format!("{line}\n"))
                    .collect::<String>()
            };
            let base = join(&base_lines);
            let canonical = join(&canonical_lines);
            let typed_text = join(&operator_lines);
            // The operator's whole change as one splice per typed code point.
            let ops = splices_between(
                &base.chars().collect::<Vec<_>>(),
                &typed_text.chars().collect::<Vec<_>>(),
            );
            let mut edits = Vec::new();
            for op in ops {
                edits.push(edit(op.offset_code_points, op.delete_code_points, ""));
                edits.extend(typed(op.offset_code_points, &op.insert));
            }
            let captured = batch(&base, edits);
            assert_eq!(captured.resulting_text, typed_text, "case {case}");
            let out = rebase(&base, &canonical, &captured)
                .unwrap_or_else(|error| panic!("case {case}: {error}\nbase:\n{base}\ncanonical:\n{canonical}\nyours:\n{typed_text}"));
            assert_eq!(out.resulting_text, join(&merged_lines), "case {case}");
            let mut replay: Vec<char> = canonical.chars().collect();
            for edit in &out.edits {
                apply(&mut replay, edit).unwrap();
            }
            assert_eq!(
                replay.iter().collect::<String>(),
                out.resulting_text,
                "case {case}"
            );
        }
    }

    /// Replay `answer` on `canonical` and check it reproduces its text.
    fn replays_on(canonical: &str, answer: &CapturedSpliceBatch) -> String {
        let mut replay: Vec<char> = canonical.chars().collect();
        for edit in &answer.edits {
            apply(&mut replay, edit).unwrap();
        }
        let replay: String = replay.iter().collect();
        assert_eq!(
            replay, answer.resulting_text,
            "splices are canonical-relative"
        );
        replay
    }

    /// `#lagcanonicalrollforward` (live 2026-10-26 13:48-13:53, agent-doc-bugs.md).
    /// The editor buffer failed the component-structure guard, so the live relay
    /// quarantined every update and the settled shadow never advanced, while the
    /// durable op path still ingested the operator's earlier keystrokes into
    /// canonical. Each re-registration then met three distinct generations:
    /// canonical held a prefix of the operator's text, the buffer held all of it.
    /// The merge refused ("canonical already carries captured operator text"),
    /// the containment proof failed (the later typing is not in canonical), and
    /// the hold could never change its answer. The buffer already carries every
    /// canonical change, so it is the lossless answer: editor text as the base,
    /// nothing rolled back toward the stale shadow.
    #[test]
    fn a_canonical_that_lags_the_cut_by_operator_typing_rolls_the_cut_forward() {
        // A rewritten line whose first keystrokes canonical ingested.
        let shadow = "<!-- agent:queue -->\ninvestigate the stale replica wedge on bugs\n<!-- /agent:queue -->\n";
        let canonical = shadow.replace(
            "investigate the stale replica wedge on bugs",
            "root-cause the",
        );
        let buffer = shadow.replace(
            "investigate the stale replica wedge on bugs",
            "root-cause the attach hold after the fence",
        );
        let answer = rebase(shadow, &canonical, &single(shadow, &buffer))
            .expect("the cut carries every canonical change");
        assert_eq!(
            replays_on(&canonical, &answer),
            buffer,
            "editor text is the base"
        );

        // The structure fault: the operator removed the loop close marker and
        // typed in its place; canonical ingested the typing, not the removal.
        let shadow = "a\n<!-- agent:loop -->\nwatch row\n<!-- /agent:loop -->\n## Todo\nb\n";
        let canonical = shadow.replacen(
            "<!-- /agent:loop -->\n",
            "- typed\n<!-- /agent:loop -->\n",
            1,
        );
        let buffer = shadow.replacen("<!-- /agent:loop -->\n", "- typed more\n", 1);
        let answer = rebase(shadow, &canonical, &single(shadow, &buffer))
            .expect("the cut carries every canonical change");
        assert_eq!(replays_on(&canonical, &answer), buffer);
        assert_eq!(
            answer.resulting_text.matches("- typed").count(),
            1,
            "never inserted twice"
        );

        // The registration path asks the same rebase.
        let registration =
            crate::captured_splice_request::registration_rebase(shadow, &buffer, &canonical)
                .expect("registration merge-forward answers the cut");
        assert_eq!(registration.resulting_text, buffer);

        // A canonical change the cut lacks still refuses: a controller response
        // beside the ingested typing is not in the buffer.
        let with_response = canonical.replacen("## Todo\n", "## Todo\n### Re: response\n", 1);
        let refused = rebase(shadow, &with_response, &single(shadow, &buffer));
        assert!(
            refused
                .as_ref()
                .map(|batch| batch.resulting_text.contains("### Re: response"))
                .unwrap_or(true),
            "a canonical change the cut lacks is never dropped: {refused:?}"
        );
    }

    /// `#lagcanonicalrollforward`, the operator sequence behind the 2026-10-26
    /// 14:56 docs.md hold: in one edit window the operator deleted a queue row
    /// and pasted part of its prompt into an existing row. Claim edit-following
    /// (`queue_claim_followed_edit previous_bytes=228 current_bytes=490`) then
    /// claimed the grown row, and the claim projection put `🟡 [#subagent:x#1] `
    /// on it in canonical while the operator kept typing on that same row
    /// (delivery was deferred behind the typing). Canonical also ingested the
    /// delete and the paste through the durable op path. Re-registration met the
    /// stale settled shadow, a buffer with the later typing and no marker, and a
    /// canonical with the marker and without the typing: the line merge overlaps
    /// on the edited row and the containment proof fails. The marker is
    /// binary-owned projection the controller re-projects from its claim; every
    /// operator change in canonical is in the buffer, so the buffer is lossless.
    #[test]
    fn a_deleted_row_pasted_into_a_claimed_row_rolls_the_cut_forward() {
        let shadow = concat!(
            "<!-- agent:queue subagents -->\n",
            "- [#docsnav] fix the docs navigation\n",
            "- [#docsapi] document the CLI flags for PR 65\n",
            "<!-- /agent:queue -->\n",
        );
        let merged_row = "- [#docsnav] fix the docs navigation and document the CLI flags";
        let pasted = shadow.replacen(
            "- [#docsnav] fix the docs navigation\n- [#docsapi] document the CLI flags for PR 65\n",
            &format!("{merged_row}\n"),
            1,
        );
        let canonical = pasted.replacen("- [#docsnav]", "- 🟡 [#subagent:pr65cli#1] [#docsnav]", 1);
        let buffer = pasted.replacen(merged_row, &format!("{merged_row} with examples"), 1);
        let whole = single(shadow, &buffer);

        let answer = rebase(shadow, &canonical, &whole)
            .expect("the claim marker is projection, every operator change is in the cut");
        assert_eq!(
            replays_on(&canonical, &answer),
            buffer,
            "editor text is the base"
        );
        assert!(
            !answer.resulting_text.contains("[#docsapi]"),
            "the deleted row stays deleted"
        );
        assert_eq!(
            answer
                .resulting_text
                .matches("document the CLI flags")
                .count(),
            1
        );
        let registration =
            crate::captured_splice_request::registration_rebase(shadow, &buffer, &canonical)
                .expect("registration merge-forward answers the cut");
        assert_eq!(registration.resulting_text, buffer);

        // Operator text in canonical the cut lacks is never dropped: another
        // queue row the controller appended still refuses.
        let appended = canonical.replacen(
            "<!-- /agent:queue -->",
            "- follow-up from the controller\n<!-- /agent:queue -->",
            1,
        );
        let refused = rebase(shadow, &appended, &whole);
        assert!(
            refused
                .as_ref()
                .map(|batch| batch
                    .resulting_text
                    .contains("follow-up from the controller"))
                .unwrap_or(true),
            "{refused:?}"
        );
    }

    /// The dual proof is exact: a canonical deletion counts as carried only
    /// when the cut deleted the same base range.
    #[test]
    fn the_dual_proof_never_resurrects_text_canonical_deleted() {
        let shadow = "alpha\nbeta\ngamma\n";
        // Canonical deleted `beta`; the cut kept it and typed elsewhere.
        let canonical = "alpha\ngamma\n";
        let buffer = "alpha\nbeta\ngamma\ndelta\n";
        let answer = rebase(shadow, canonical, &single(shadow, buffer));
        assert!(
            answer
                .as_ref()
                .map(|batch| batch.resulting_text != buffer)
                .unwrap_or(true),
            "the cut never wins over a canonical deletion it does not carry: {answer:?}"
        );
        // An unrelated rewrite of one line by both sides is not a continuation.
        let shadow = "q\nold line\nz\n";
        let canonical = "q\nok\nz\n";
        let buffer = "q\nI looked into it\nz\n";
        let answer = rebase(shadow, canonical, &single(shadow, buffer));
        assert!(
            answer
                .as_ref()
                .map(|batch| batch.resulting_text != buffer)
                .unwrap_or(true),
            "{answer:?}"
        );
    }

    fn single(before: &str, after: &str) -> CapturedSpliceBatch {
        crate::captured_splice_request::single_splice(before, after)
    }

    /// `#captureddeltalivelock` (live 2026-10-15, agent-doc-bugs.md). The plugin
    /// replica applied an operator burst that rewrote a queue line, the durable
    /// push timed out, and the controller ingested the frame anyway. The plugin
    /// kept its shadow at the pre-burst text while the operator kept typing on
    /// the same line, then the controller appended a response. Rebasing the
    /// whole retained stream from the stale shadow must keep refusing: canonical
    /// already carries part of the operator's text, and this planner cannot tell
    /// a lost receipt from a duplicate. The plugin's exit is exact instead: the
    /// containment proof settles the burst, and only the later typing rebases
    /// from the burst's result onto canonical, keeping the response and every
    /// operator character exactly once.
    #[test]
    fn settled_burst_rebases_only_later_typing_over_a_response() {
        let shadow = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: earlier\n",
            "done\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:queue -->\n",
            "investigate the stale replica wedge on bugs\n",
            "<!-- /agent:queue -->\n",
        );
        let line = "investigate the stale replica wedge on bugs";
        let at = shadow.find(line).unwrap();
        let line_start = shadow[..at].chars().count();
        // The ingested burst: delete the old line text, type a new one.
        let burst = batch(
            shadow,
            vec![
                edit(line_start, line.chars().count(), ""),
                edit(line_start, 0, "root-cause the"),
            ],
        );
        let settled = burst.resulting_text.clone();
        // Later typing, same line, never acknowledged anywhere.
        let tail_at = line_start + "root-cause the".chars().count();
        let later = batch(&settled, vec![edit(tail_at, 0, " captured-delta livelock")]);
        let visible = later.resulting_text.clone();
        let mut whole_edits = burst.edits.clone();
        whole_edits.extend(later.edits.clone());
        let whole = batch(shadow, whole_edits);
        assert_eq!(whole.resulting_text, visible);

        let response = "### Re: wedge\nfixed\n";
        let canonical = settled.replace(
            "<!-- /agent:exchange -->",
            &format!("{response}<!-- /agent:exchange -->"),
        );

        let refusal = rebase(shadow, &canonical, &whole)
            .expect_err("the stale-shadow rebase of the whole stream refuses (the livelock)");
        assert!(
            format!("{refusal:#}").contains("canonical already carries captured operator text"),
            "{refusal:#}"
        );

        assert!(
            canonical_contains_captured(shadow, &canonical, &burst).unwrap(),
            "the settle proof: canonical carries the unacknowledged burst"
        );
        assert!(
            !canonical_contains_captured(shadow, &canonical, &whole).unwrap(),
            "the later typing is not in canonical"
        );
        // A canonical that never received the burst proves nothing.
        let lost = shadow.replace(
            "<!-- /agent:exchange -->",
            &format!("{response}<!-- /agent:exchange -->"),
        );
        assert!(!canonical_contains_captured(shadow, &lost, &burst).unwrap());

        let out = rebase(&settled, &canonical, &later)
            .expect("only the later typing rebases from the settled baseline");
        let expected = visible.replace(
            "<!-- /agent:exchange -->",
            &format!("{response}<!-- /agent:exchange -->"),
        );
        assert_eq!(out.resulting_text, expected);
        assert_eq!(out.resulting_text.matches("root-cause the").count(), 1);
        assert_eq!(out.resulting_text.matches("### Re: wedge").count(), 1);
        let mut replay: Vec<char> = canonical.chars().collect();
        for edit in &out.edits {
            apply(&mut replay, edit).unwrap();
        }
        assert_eq!(replay.iter().collect::<String>(), expected);
    }

    /// The operator's whole change as one splice per deleted run and one per
    /// typed code point, the shape the JetBrains plugin captures.
    fn captured_typing(base: &str, typed_text: &str) -> CapturedSpliceBatch {
        let ops = splices_between(
            &base.chars().collect::<Vec<_>>(),
            &typed_text.chars().collect::<Vec<_>>(),
        );
        let mut edits = Vec::new();
        for op in ops {
            edits.push(edit(op.offset_code_points, op.delete_code_points, ""));
            edits.extend(typed(op.offset_code_points, &op.insert));
        }
        let captured = batch(base, edits);
        assert_eq!(captured.resulting_text, typed_text);
        captured
    }

    /// `#mergecapturedsplice` (found by the `#jetbrainsreplicaloops` subagent,
    /// sdk.md 2026-10-16). The operator deleted a struck queue line and typed
    /// new items while a closeout appended ` (closed)` inside that same line.
    /// The merge-forward reconcile diffs char by char, aligned the deleted line
    /// with the operator's new one (both start `- ` and end in a newline), and
    /// re-attached the close marker to the operator's text:
    /// `- review [#retryloop] (closed)`. An insertion whose anchor the operator
    /// deleted is an overlap: the planner refuses, the plugin keeps the batch,
    /// and the controller-licensed roll-forward settles the visible text.
    #[test]
    fn canonical_insert_inside_a_line_the_operator_deleted_refuses_instead_of_reanchoring() {
        let base = concat!(
            "## Exchange\n",
            "<!-- agent:exchange -->\n",
            "What is the full todo text for #tdc?\n",
            "<!-- agent:boundary:5d1f0a2c -->\n",
            "<!-- /agent:exchange -->\n",
            "## Queue\n",
            "<!-- agent:queue -->\n",
            "- ~~do [#tdc]~~\n",
            "- do [#sdkdocs]\n",
            "<!-- /agent:queue -->\n",
        );
        let canonical = base.replace("- ~~do [#tdc]~~\n", "- ~~do [#tdc]~~ (closed)\n");
        let queue = "- ~~do [#tdc]~~\n- do [#sdkdocs]\n";
        for operator_queue in [
            // The reported fixture: struck line removed, two items typed.
            "- do [#sdkdocs]\n- review [#retryloop]\n- do [#sdkexamples]\n",
            "- do [#sdkdocs]\n- review [#retryloop]\n",
            // The struck line replaced in place by a new item.
            "- review [#retryloop]\n- do [#sdkdocs]\n",
        ] {
            let visible = base.replace(queue, operator_queue);
            let out = rebase(base, &canonical, &captured_typing(base, &visible));
            let refusal = match out {
                Err(refusal) => format!("{refusal:#}"),
                Ok(out) => panic!(
                    "a canonical insert whose anchor the operator deleted must refuse, \
                     never land on operator text:\n{}",
                    out.resulting_text
                ),
            };
            assert!(refusal.contains("merge-forward refused"), "{refusal}");
        }

        // Control: the same burst beside a close of a line the operator kept
        // still merges forward, and every line is one side's line verbatim.
        let visible = base.replace(
            queue,
            "- do [#sdkdocs]\n- review [#retryloop]\n- do [#sdkexamples]\n",
        );
        let closed_kept_line = base.replace("- do [#sdkdocs]\n", "- do [#sdkdocs] (closed)\n");
        let out = rebase(base, &closed_kept_line, &captured_typing(base, &visible))
            .expect("a close of a line the operator kept is independent");
        assert_eq!(
            out.resulting_text,
            visible.replace("- do [#sdkdocs]\n", "- do [#sdkdocs] (closed)\n")
        );
        let mut replay: Vec<char> = closed_kept_line.chars().collect();
        for edit in &out.edits {
            apply(&mut replay, edit).unwrap();
        }
        assert_eq!(replay.iter().collect::<String>(), out.resulting_text);

        // Control: an anchor that survives. The operator edited the item's
        // verb, keeping the line recognisably the same, and canonical closed
        // it at the other end; the exact translation keeps both.
        let edited = base.replace("- do [#sdkdocs]\n", "- review [#sdkdocs]\n");
        let out = rebase(base, &closed_kept_line, &captured_typing(base, &edited))
            .expect("a canonical change on a line the operator edited in place keeps its anchor");
        assert_eq!(
            out.resulting_text,
            base.replace("- do [#sdkdocs]\n", "- review [#sdkdocs] (closed)\n")
        );

        // The document edge is an anchor no edit removes: a canonical line
        // inserted at the top survives the operator deleting the first line.
        let top = "- ~~do [#tdc]~~\n- do [#sdkdocs]\n";
        assert!(!canonical_change_lost_its_anchor(
            top,
            &format!("## Queue\n{top}"),
            "- do [#sdkdocs]\n"
        ));
        assert!(canonical_change_lost_its_anchor(
            top,
            &top.replace("~~\n", "~~\n## Moved\n"),
            "- review [#retryloop]\n"
        ));
    }

    /// The batch a re-registering JetBrains editor derives from its settled
    /// shadow and live buffer (`singleSpliceBatchUtil`): one splice keeping the
    /// common code-point prefix and suffix.
    fn single_splice_batch(before: &str, after: &str) -> CapturedSpliceBatch {
        let (old, new): (Vec<char>, Vec<char>) =
            (before.chars().collect(), after.chars().collect());
        let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
        let suffix = old[prefix..]
            .iter()
            .rev()
            .zip(new[prefix..].iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        let insert: String = new[prefix..new.len() - suffix].iter().collect();
        let delete = old.len() - prefix - suffix;
        let edits = if delete == 0 && insert.is_empty() {
            Vec::new()
        } else {
            vec![edit(prefix, delete, &insert)]
        };
        let captured = batch(before, edits);
        assert_eq!(captured.resulting_text, after);
        captured
    }

    /// The registration merge the JetBrains plugin computes
    /// (`cleanRegistrationMergeUtil`): the captured-splice rebase of the one
    /// splice from the settled shadow to the live buffer.
    fn registration_merge(shadow: &str, buffer: &str, canonical: &str) -> Result<String> {
        rebase(shadow, canonical, &single_splice_batch(shadow, buffer))
            .map(|answer| answer.resulting_text)
    }

    /// `#jetbrainscleanregistrati` (sdk.md 2026-10-16). JetBrains
    /// `cleanRegistrationMerge` asked the char-level reconcile first: the
    /// operator deleted the struck line `- ~~do [#tdc]~~` and typed new items
    /// while canonical closed the kept `- do [#sdkdocs]`, and the char diff
    /// aligned base `- do [#sdkdocs]` with the operator's typed
    /// `- do [#sdkexamples]` (`- do [#sdk`, `s]`, the newline), so the close
    /// marker landed on operator text with zero conflicts reported. The
    /// registration merge is now the captured-splice rebase alone, which holds
    /// whole-line anchoring: the close stays on the line it closed.
    #[test]
    fn registration_merge_never_reanchors_a_close_onto_a_typed_queue_item() {
        let base = concat!(
            "## Queue\n",
            "<!-- agent:queue -->\n",
            "- ~~do [#tdc]~~\n",
            "- do [#sdkdocs]\n",
            "<!-- /agent:queue -->\n",
        );
        let queue = "- ~~do [#tdc]~~\n- do [#sdkdocs]\n";
        let buffer = base.replace(
            queue,
            "- do [#sdkdocs]\n- review [#retryloop]\n- do [#sdkexamples]\n",
        );
        let canonical = base.replace("- do [#sdkdocs]\n", "- do [#sdkdocs] (closed)\n");
        let expected = buffer.replace("- do [#sdkdocs]\n", "- do [#sdkdocs] (closed)\n");

        // The char-level reconcile the plugin used to try first: a corrupted
        // merge reported clean. The whole-line check refuses it.
        let char_level = crate::conflict_reconcile::reconcile(base, &buffer, &canonical, None)
            .expect("the char-level reconcile answers");
        assert_eq!(char_level.conflicts, 0, "{}", char_level.text);
        assert!(
            char_level.text.contains("- do [#sdkexamples] (closed)\n"),
            "fixture no longer reproduces the reported re-anchoring:\n{}",
            char_level.text
        );
        assert!(!clean_merge_keeps_line_anchors(
            base,
            &buffer,
            &canonical,
            &char_level.text
        ));

        // The merge the plugin now asks for: the close stays on its own line.
        let merged = registration_merge(base, &buffer, &canonical)
            .expect("a close of a line the operator kept is independent of the typed items");
        assert_eq!(merged, expected);
        assert!(!merged.contains("[#sdkexamples] (closed)"), "{merged}");
        assert!(clean_merge_keeps_line_anchors(
            base, &buffer, &canonical, &merged
        ));

        // The deleted-anchor variant with replacement typing: canonical closes
        // the struck line the operator deleted. There is no line to carry the
        // close, so the registration merge refuses (the plugin then holds the
        // operator's buffer) instead of attaching it to a typed item. Pure
        // pruning of already-struck rows is covered separately below.
        let closed_struck = base.replace("- ~~do [#tdc]~~\n", "- ~~do [#tdc]~~ (closed)\n");
        for operator_queue in [
            "- do [#sdkdocs]\n- review [#retryloop]\n- do [#sdkexamples]\n",
            "- review [#retryloop]\n- do [#sdkdocs]\n",
        ] {
            let buffer = base.replace(queue, operator_queue);
            if let Ok(merged) = registration_merge(base, &buffer, &closed_struck) {
                panic!("a close whose anchor line the operator deleted must refuse:\n{merged}");
            }
            if let Ok(char_level) =
                crate::conflict_reconcile::reconcile(base, &buffer, &closed_struck, None)
                && char_level.conflicts == 0
            {
                assert!(
                    !clean_merge_keeps_line_anchors(
                        base,
                        &buffer,
                        &closed_struck,
                        &char_level.text
                    ),
                    "{}",
                    char_level.text
                );
            }
        }

        // Control: an independent edit on another line still merges cleanly
        // through both, and the two agree.
        let edited_heading = buffer.replace("## Queue\n", "## Queue (next)\n");
        let merged = registration_merge(base, &edited_heading, &canonical).unwrap();
        assert_eq!(merged, expected.replace("## Queue\n", "## Queue (next)\n"));
    }

    /// `#struckqueuepruneattach` (sdk-demo.md 2026-10-19). Immediately after
    /// the controller struck queue work, the operator deleted the terminal row.
    /// Canonical advanced that same row while retaining another live prompt,
    /// so the generic deleted-anchor guard held registration forever as
    /// `ambiguous-retained-projection`. Deleting an already-settled struck row
    /// is a keyed queue tombstone, not an ambiguous rewrite: prune only that
    /// key from canonical and retain both the live head and response.
    #[test]
    fn registration_merge_prunes_only_settled_struck_queue_rows() {
        let base = concat!(
            "# SDK demo\n",
            "<!-- agent:exchange -->\n",
            "settled response\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:queue id=issues -->\n",
            "- ~~investigate attach [#retired]~~\n",
            "- ~~document prior attach [#retired2]~~\n",
            "- watch https://github.com/btakita/agent-doc/issues\n",
            "<!-- /agent:queue -->\n",
        );
        let buffer = base.replace(
            concat!(
                "- ~~investigate attach [#retired]~~\n",
                "- ~~document prior attach [#retired2]~~\n",
            ),
            "",
        );
        let canonical = base
            .replace(
                "settled response\n",
                "settled response\n\n### Re: retained attach\n\nRecorded.\n",
            )
            .replace(
                "- ~~investigate attach [#retired]~~\n",
                "- ~~investigate attach [#retired]~~ (closed)\n",
            )
            .replace(
                "- ~~document prior attach [#retired2]~~\n",
                "- ~~document prior attach [#retired2]~~ (closed)\n",
            );
        let expected = canonical
            .replace("- ~~investigate attach [#retired]~~ (closed)\n", "")
            .replace("- ~~document prior attach [#retired2]~~ (closed)\n", "");

        let answer = rebase(base, &canonical, &single_splice_batch(base, &buffer))
            .expect("deleting only a settled struck queue row is unambiguous");
        assert_eq!(answer.resulting_text, expected);
        assert!(
            answer
                .resulting_text
                .contains("https://github.com/btakita/agent-doc/issues")
        );
        assert!(answer.resulting_text.contains("### Re: retained attach"));
        assert!(!answer.resulting_text.contains("[#retired]"));
        assert!(!answer.resulting_text.contains("[#retired2]"));
        let mut replay: Vec<char> = canonical.chars().collect();
        for edit in &answer.edits {
            apply(&mut replay, edit).unwrap();
        }
        assert_eq!(replay.iter().collect::<String>(), expected);

        // Deleting a live row that canonical concurrently edits remains an
        // overlap; the semantic exception never tombstones live work.
        let live_deleted =
            base.replace("- watch https://github.com/btakita/agent-doc/issues\n", "");
        let live_advanced = base.replace(
            "- watch https://github.com/btakita/agent-doc/issues\n",
            "- watch https://github.com/btakita/agent-doc/issues (pinned)\n",
        );
        assert!(
            rebase(
                base,
                &live_advanced,
                &single_splice_batch(base, &live_deleted)
            )
            .is_err()
        );

        // Deleting the struck row while typing replacement work still has no
        // anchor for canonical's close decoration and remains held.
        let replaced = buffer.replace(
            "- watch https://github.com/btakita/agent-doc/issues\n",
            concat!(
                "- watch https://github.com/btakita/agent-doc/issues\n",
                "- investigate another attach [#new]\n",
            ),
        );
        let closed_struck = base.replace(
            "- ~~investigate attach [#retired]~~\n",
            "- ~~investigate attach [#retired]~~ (closed)\n",
        );
        assert!(rebase(base, &closed_struck, &single_splice_batch(base, &replaced)).is_err());
    }

    /// `#mergecapturedsplice` property: no canonical change ever lands inside
    /// or on text the operator inserted unless its anchor line survived. The
    /// operator keeps, deletes, rewrites (similar or unrelated text), or types
    /// a line after each base line; canonical keeps, closes, strikes, or
    /// inserts a response block after it. Whenever both sides changed one base
    /// line the planner must refuse. Any answer it gives must be exactly the
    /// line-wise union (so no line is a hybrid of the two sides, and every
    /// operator line survives verbatim, once) and must replay on canonical.
    /// Deterministic xorshift, no new deps.
    #[test]
    fn a_canonical_change_never_lands_on_operator_text_once_its_anchor_line_is_gone() {
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let mut next = move |bound: usize| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % bound as u64) as usize
        };
        let (mut answered, mut refused, mut same_line) = (0, 0, 0);
        let (mut registration_answered, mut registration_refused) = (0, 0);
        let mut char_level_reanchored = 0;
        for case in 0..400 {
            let lines = 3 + next(8);
            let base_lines: Vec<String> = (0..lines).map(|n| format!("- do [#item{n}]")).collect();
            let (mut canonical_lines, mut operator_lines, mut merged_lines) =
                (Vec::new(), Vec::new(), Vec::new());
            let mut both_changed_a_line = false;
            for (n, line) in base_lines.iter().enumerate() {
                let canonical_op = next(4);
                let mut operator_op = next(4);
                if canonical_op == 3 && operator_op == 3 {
                    // Both inserting after one line: the same anchor, which
                    // the planner already refuses; outside this property.
                    operator_op = 0;
                }
                both_changed_a_line |=
                    matches!(canonical_op, 1 | 2) && matches!(operator_op, 1 | 2);
                let canonical_line = match canonical_op {
                    1 => Some(format!("{line} (closed)")),
                    2 => Some(line.replace("- do", "- ~~do") + "~~"),
                    _ => None,
                };
                let operator_line = match operator_op {
                    1 => Some(None),
                    2 if next(2) == 0 => Some(Some(format!("{line} operator {case} edit"))),
                    2 => Some(Some(format!("- review [#op{case}x{n}]"))),
                    _ => None,
                };
                canonical_lines.push(canonical_line.clone().unwrap_or_else(|| line.clone()));
                match &operator_line {
                    Some(None) => {}
                    Some(Some(rewritten)) => operator_lines.push(rewritten.clone()),
                    None => operator_lines.push(line.clone()),
                }
                match (operator_line, canonical_line) {
                    (Some(None), _) => {}
                    (Some(Some(rewritten)), _) => merged_lines.push(rewritten),
                    (None, Some(changed)) => merged_lines.push(changed),
                    (None, None) => merged_lines.push(line.clone()),
                }
                if canonical_op == 3 {
                    let block = [
                        format!("### Re: item{n}"),
                        String::new(),
                        format!("answer {case} {n}"),
                    ];
                    canonical_lines.extend(block.iter().cloned());
                    merged_lines.extend(block);
                }
                if operator_op == 3 {
                    let typed_line = format!("- review [#typed{case}x{n}]");
                    operator_lines.push(typed_line.clone());
                    merged_lines.push(typed_line);
                }
            }
            let join = |lines: &[String]| {
                lines
                    .iter()
                    .map(|line| format!("{line}\n"))
                    .collect::<String>()
            };
            let base = join(&base_lines);
            let canonical = join(&canonical_lines);
            let yours = join(&operator_lines);
            let captured = captured_typing(&base, &yours);
            same_line += usize::from(both_changed_a_line);
            // `#jetbrainscleanregistrati`: the JetBrains registration merge
            // rebases the one splice from shadow to buffer, a different batch
            // shape than per-keystroke typing; it is held to the same answer.
            let expected = join(&merged_lines);
            match registration_merge(&base, &yours, &canonical) {
                Err(_) => registration_refused += 1,
                Ok(merged) => {
                    registration_answered += 1;
                    assert!(
                        !both_changed_a_line,
                        "case {case}: registration merged a line both sides changed\n{merged}"
                    );
                    assert_eq!(
                        merged, expected,
                        "case {case} (registration)\nbase:\n{base}\ncanonical:\n{canonical}\nyours:\n{yours}"
                    );
                }
            }
            // The char-level reconcile is admitted (over FFI) only when it
            // keeps whole-line anchoring; anything admitted is the same answer.
            if let Ok(char_level) =
                crate::conflict_reconcile::reconcile(&base, &yours, &canonical, None)
                && char_level.conflicts == 0
            {
                if clean_merge_keeps_line_anchors(&base, &yours, &canonical, &char_level.text) {
                    assert!(
                        !both_changed_a_line,
                        "case {case}: admitted a char-level merge of a line both changed\n{}",
                        char_level.text
                    );
                    assert_eq!(
                        char_level.text, expected,
                        "case {case} (char-level admitted)"
                    );
                } else if !both_changed_a_line && char_level.text != expected {
                    char_level_reanchored += 1;
                }
            }
            match rebase(&base, &canonical, &captured) {
                Err(_) => refused += 1,
                Ok(out) => {
                    answered += 1;
                    assert!(
                        !both_changed_a_line,
                        "case {case}: both sides changed one line, yet the planner answered\n\
                         base:\n{base}\ncanonical:\n{canonical}\nyours:\n{yours}\nmerged:\n{}",
                        out.resulting_text
                    );
                    assert_eq!(
                        out.resulting_text,
                        join(&merged_lines),
                        "case {case}\nbase:\n{base}\ncanonical:\n{canonical}\nyours:\n{yours}"
                    );
                    let mut replay: Vec<char> = canonical.chars().collect();
                    for edit in &out.edits {
                        apply(&mut replay, edit).unwrap();
                    }
                    assert_eq!(
                        replay.iter().collect::<String>(),
                        out.resulting_text,
                        "case {case}"
                    );
                }
            }
        }
        // Non-vacuity: the generator reaches both outcomes and the bug class.
        assert!(answered >= 40, "answered {answered}, refused {refused}");
        assert!(same_line >= 100, "same-line cases {same_line}");
        assert!(
            registration_answered >= 40,
            "registration answered {registration_answered}, refused {registration_refused}"
        );
        // The generator reaches the char-level re-anchoring the FFI now refuses.
        assert!(
            char_level_reanchored >= 1,
            "char-level re-anchored {char_level_reanchored}"
        );
        eprintln!(
            "answered {answered} refused {refused} same-line {same_line} registration \
             answered {registration_answered} refused {registration_refused} char-level \
             reanchored {char_level_reanchored}"
        );
    }
}
