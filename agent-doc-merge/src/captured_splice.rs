//! Rebase a captured editor splice stream over independent canonical changes.
//! This pure command planner never chooses a winner for overlapping edits.

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use similar::{Algorithm, DiffTag, capture_diff_slices};

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
    let mut current: Vec<char> = canonical.chars().collect();
    // Another editor observation can publish this same burst before recovery,
    // while canonical also advances elsewhere. Prove every net captured change
    // against the same base rather than requiring whole-document equality.
    let captured_changes = capture_diff_slices(Algorithm::Myers, &old, &captured_result);
    let canonical_changes = capture_diff_slices(Algorithm::Myers, &old, &current);
    if captured_changes
        .iter()
        .filter(|op| op.tag() != DiffTag::Equal)
        .all(|captured| {
            canonical_changes.iter().any(|observed| {
                observed.tag() != DiffTag::Equal
                    && observed.old_range() == captured.old_range()
                    && current[observed.new_range()] == captured_result[captured.new_range()]
            })
        })
    {
        return Ok(CapturedSpliceBatch {
            edits: Vec::new(),
            resulting_text: canonical.to_owned(),
        });
    }
    let mut rebased = Vec::with_capacity(batch.edits.len());
    for edit in &batch.edits {
        let start = edit.offset_code_points;
        let end = start + edit.delete_code_points;
        let ops = capture_diff_slices(Algorithm::Myers, &old, &current);
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

/// `#ambiguousholdforever`: prove at editor registration that `canonical`
/// already carries every change the captured batch made to `base`, including
/// when the controller inserted its own text directly beside the operator's
/// (Myers then reports one merged insertion, which [`rebase`] rightly refuses to
/// treat as applied on the hot typing path). Each captured change must match a
/// canonical change over the same base range whose inserted text begins or ends
/// with the operator's inserted text. Registration-only: adopting canonical is
/// then lossless, where the alternative was refusing the endpoint indefinitely.
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
    let captured_changes = capture_diff_slices(Algorithm::Myers, &old, &captured);
    let canonical_changes = capture_diff_slices(Algorithm::Myers, &old, &current);
    Ok(captured_changes
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
        }))
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
        let edits = batch(shadow, vec![edit(shadow[..prefix].chars().count(), 0, pasted)]);
        assert_eq!(edits.resulting_text, buffer);
        assert!(canonical_contains_captured(shadow, canonical, &edits).unwrap());

        // Control: canonical WITHOUT the paste does not contain it.
        let without_paste = canonical.replace("```\n• Failed (exit 1) write --commit\n```\n", "");
        assert!(!canonical_contains_captured(shadow, &without_paste, &edits).unwrap());
        // Control: a different operator edit at the same anchor is not contained.
        let other = batch(shadow, vec![edit(shadow[..prefix].chars().count(), 0, "other text\n")]);
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
}
