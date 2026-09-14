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
