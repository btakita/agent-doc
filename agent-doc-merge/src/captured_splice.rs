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
/// edit. Any overlap fails before a replacement replica is registered.
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
    if windows.iter().any(|window| window.canonical_ops > 0) {
        return merge_windows(&captured_result, &current, &canonical_changes, &windows);
    }
    match translate_edits(&old, &current, batch) {
        Ok(rebased) => Ok(rebased),
        // A splice inside the burst may touch a canonical change even when
        // the burst's net change does not; the net change still translates.
        Err(_) => merge_windows(&captured_result, &current, &canonical_changes, &windows),
    }
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
/// The JetBrains plugin's `withoutBinaryOwnedMarkersUtil` applies the same rule.
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
}
