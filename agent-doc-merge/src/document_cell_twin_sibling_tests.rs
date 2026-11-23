//! Two byte-identical rows under ONE parent. A strike or edit
//! one side makes to one copy lands on that copy, never on its twin.
//! (`#e107moved` covered identical rows under different parents.)

use super::{merge_3way, merge_3way_with_ops};
use crate::crdt::{CrdtDoc, EditorOp, merge_by_component};

const HEAD: &str = "- ### rebase the release branch onto main [#q-0a1b2c3d4e5f]\n";
const OTHER: &str = "- ### update the changelog [#q-6f5e4d3c2b1a]\n";
const REBASE: &str = "  - #rebase\n";
const STRUCK: &str = "  - ~~#rebase~~\n";
const NOTE: &str = "  - check CI first\n";
const TYPED: &str = "- operator typed a new row\n";

fn doc(rows: &[&str]) -> String {
    format!(
        "## Run <!-- agent:run id=\"main\" subagents -->\n{}<!-- /agent:run -->\n",
        rows.concat()
    )
}

/// Every merge path and order: the per-cell 3-way merge (when it does not
/// decline) and the component merge, each with either side as `ours`.
fn merged_every_order(base: &str, a: &str, b: &str) -> Vec<(String, String)> {
    let state = CrdtDoc::from_text(base).encode_state();
    let mut out = vec![
        (
            "merge_by_component(a, b)".to_string(),
            merge_by_component(Some(&state), a, b).expect("merge a over b"),
        ),
        (
            "merge_by_component(b, a)".to_string(),
            merge_by_component(Some(&state), b, a).expect("merge b over a"),
        ),
    ];
    for (label, ours, theirs) in [("merge_3way(a, b)", a, b), ("merge_3way(b, a)", b, a)] {
        let outcome = merge_3way(base, ours, theirs);
        if !outcome.fell_back {
            assert!(
                outcome.conflicts.is_empty(),
                "{label}: unexpected conflicts {:?}",
                outcome.conflicts
            );
            out.push((label.to_string(), outcome.merged_text));
        }
    }
    out
}

fn assert_every_order(base: &str, a: &str, b: &str, expected: &str) {
    for (label, merged) in merged_every_order(base, a, b) {
        assert_eq!(merged, expected, "{label}");
    }
}

/// `queue consume` strikes the SECOND `#rebase` while the operator types a
/// row elsewhere: exactly the second copy is struck.
#[test]
fn striking_the_second_twin_strikes_only_the_second() {
    let base = doc(&[HEAD, REBASE, REBASE, OTHER]);
    let agent = doc(&[HEAD, REBASE, STRUCK, OTHER]);
    let editor = doc(&[HEAD, REBASE, REBASE, OTHER, TYPED]);
    let expected = doc(&[HEAD, REBASE, STRUCK, OTHER, TYPED]);
    assert_every_order(&base, &agent, &editor, &expected);
}

/// The FIRST copy struck: only the first copy is struck.
#[test]
fn striking_the_first_twin_strikes_only_the_first() {
    let base = doc(&[HEAD, REBASE, REBASE, OTHER]);
    let agent = doc(&[HEAD, STRUCK, REBASE, OTHER]);
    let editor = doc(&[HEAD, REBASE, REBASE, OTHER, TYPED]);
    let expected = doc(&[HEAD, STRUCK, REBASE, OTHER, TYPED]);
    assert_every_order(&base, &agent, &editor, &expected);
}

/// The operator edits the FIRST copy while an agent write lands elsewhere:
/// only the first copy changes.
#[test]
fn editing_the_first_twin_changes_only_the_first() {
    let edited = "  - #rebase onto main\n";
    let base = doc(&[HEAD, REBASE, REBASE, OTHER]);
    let editor = doc(&[HEAD, edited, REBASE, OTHER]);
    let agent = doc(&[
        HEAD,
        REBASE,
        REBASE,
        "- ### ~~update the changelog [#q-6f5e4d3c2b1a]~~\n",
    ]);
    let expected = doc(&[
        HEAD,
        edited,
        REBASE,
        "- ### ~~update the changelog [#q-6f5e4d3c2b1a]~~\n",
    ]);
    assert_every_order(&base, &agent, &editor, &expected);
}

/// The operator edits the SECOND copy: only the second copy changes.
#[test]
fn editing_the_second_twin_changes_only_the_second() {
    let edited = "  - #rebase onto main\n";
    let base = doc(&[HEAD, REBASE, REBASE, OTHER]);
    let editor = doc(&[HEAD, REBASE, edited, OTHER]);
    let agent = doc(&[HEAD, REBASE, REBASE, OTHER, TYPED]);
    let expected = doc(&[HEAD, REBASE, edited, OTHER, TYPED]);
    assert_every_order(&base, &agent, &editor, &expected);
}

/// An agent strikes the second copy while the operator inserts a distinct
/// row between the two copies: the strike stays on the second copy and the
/// inserted row stays between them.
#[test]
fn striking_the_second_twin_survives_an_insert_between_them() {
    let base = doc(&[HEAD, REBASE, REBASE, OTHER]);
    let agent = doc(&[HEAD, REBASE, STRUCK, OTHER]);
    let editor = doc(&[HEAD, REBASE, NOTE, REBASE, OTHER]);
    let expected = doc(&[HEAD, REBASE, NOTE, STRUCK, OTHER]);
    assert_every_order(&base, &agent, &editor, &expected);
}

/// An agent strikes the first copy while the operator inserts a distinct row
/// between the two copies.
#[test]
fn striking_the_first_twin_survives_an_insert_between_them() {
    let base = doc(&[HEAD, REBASE, REBASE, OTHER]);
    let agent = doc(&[HEAD, STRUCK, REBASE, OTHER]);
    let editor = doc(&[HEAD, REBASE, NOTE, REBASE, OTHER]);
    let expected = doc(&[HEAD, STRUCK, NOTE, REBASE, OTHER]);
    assert_every_order(&base, &agent, &editor, &expected);
}

/// Two `squash and merge` replies under one PR head, the second struck, while
/// the operator removes one copy. Which copy went is ambiguous; the reading in
/// which the operator removed the copy the agent did not strike keeps both
/// changes, so the strike survives and the completed work never re-arms.
#[test]
fn deleting_the_first_twin_while_the_second_is_struck() {
    let reply = "  - squash and merge\n";
    let struck = "  - ~~squash and merge~~\n";
    let base = doc(&[HEAD, reply, reply, OTHER]);
    let agent = doc(&[HEAD, reply, struck, OTHER]);
    let editor = doc(&[HEAD, reply, OTHER]);
    let expected = doc(&[HEAD, struck, OTHER]);
    assert_every_order(&base, &agent, &editor, &expected);
}

const EDITED: &str = "  - #rebase onto main\n";

/// The 2026-11-23 shape: an agent strikes the FIRST copy while the operator
/// edits the SECOND. The edited copy leaves the group, and the copy left over
/// was matched to the last base copy, so the strike was read as landing on
/// the row the operator replaced and was lost.
#[test]
fn striking_the_first_twin_while_the_second_is_edited() {
    let base = doc(&[HEAD, REBASE, REBASE, OTHER]);
    let agent = doc(&[HEAD, STRUCK, REBASE, OTHER]);
    let editor = doc(&[HEAD, REBASE, EDITED, OTHER]);
    let expected = doc(&[HEAD, STRUCK, EDITED, OTHER]);
    assert_every_order(&base, &agent, &editor, &expected);
}

/// The second copy struck while the operator edits the first.
#[test]
fn striking_the_second_twin_while_the_first_is_edited() {
    let base = doc(&[HEAD, REBASE, REBASE, OTHER]);
    let agent = doc(&[HEAD, REBASE, STRUCK, OTHER]);
    let editor = doc(&[HEAD, EDITED, REBASE, OTHER]);
    let expected = doc(&[HEAD, EDITED, STRUCK, OTHER]);
    assert_every_order(&base, &agent, &editor, &expected);
}

/// Three copies: the middle one struck while the operator edits the last.
/// The strike landed on the FIRST copy before.
#[test]
fn striking_the_middle_of_three_twins_while_the_last_is_edited() {
    let base = doc(&[HEAD, REBASE, REBASE, REBASE, OTHER]);
    let agent = doc(&[HEAD, REBASE, STRUCK, REBASE, OTHER]);
    let editor = doc(&[HEAD, REBASE, REBASE, EDITED, OTHER]);
    let expected = doc(&[HEAD, REBASE, STRUCK, EDITED, OTHER]);
    assert_every_order(&base, &agent, &editor, &expected);
}

/// Three copies: the last one struck while the operator edits the first.
#[test]
fn striking_the_last_of_three_twins_while_the_first_is_edited() {
    let base = doc(&[HEAD, REBASE, REBASE, REBASE, OTHER]);
    let agent = doc(&[HEAD, REBASE, REBASE, STRUCK, OTHER]);
    let editor = doc(&[HEAD, EDITED, REBASE, REBASE, OTHER]);
    let expected = doc(&[HEAD, EDITED, REBASE, STRUCK, OTHER]);
    assert_every_order(&base, &agent, &editor, &expected);
}

/// Both sides strike a different copy: both copies end struck.
#[test]
fn each_side_striking_a_different_twin_strikes_both() {
    let base = doc(&[HEAD, REBASE, REBASE, OTHER]);
    let agent = doc(&[HEAD, REBASE, STRUCK, OTHER]);
    let editor = doc(&[HEAD, STRUCK, REBASE, OTHER]);
    let expected = doc(&[HEAD, STRUCK, STRUCK, OTHER]);
    assert_every_order(&base, &agent, &editor, &expected);
}

/// The operator types a THIRD identical copy among the two while an agent
/// strikes the second original. Text alone cannot say which copy is new, and
/// the two readings strike different rows with no conflict in either, so the
/// keyed cut (agent ours, editor theirs) refuses. The component merge it
/// falls back to keeps all three copies with exactly one struck.
#[test]
fn an_identical_copy_inserted_among_twins_refuses_the_keyed_cut() {
    let base = doc(&[HEAD, REBASE, REBASE, OTHER]);
    let agent = doc(&[HEAD, REBASE, STRUCK, OTHER]);
    let editor = doc(&[HEAD, REBASE, REBASE, REBASE, OTHER]);
    assert!(merge_3way(&base, &agent, &editor).fell_back);
    let state = CrdtDoc::from_text(&base).encode_state();
    let merged = merge_by_component(Some(&state), &agent, &editor).expect("component merge");
    assert_eq!(merged.matches(REBASE).count(), 2, "{merged}");
    assert_eq!(merged.matches(STRUCK).count(), 1, "{merged}");
}

/// The same insert with the editor's captured ops: the op lands in the first
/// copy's cell, so the second copy is untouched and pinned to its base row.
/// The strike lands on it, the last row.
#[test]
fn op_origin_pins_the_untouched_twin_for_an_identical_insert() {
    let base = doc(&[HEAD, REBASE, REBASE, OTHER]);
    let agent = doc(&[HEAD, REBASE, STRUCK, OTHER]);
    let editor = doc(&[HEAD, REBASE, REBASE, REBASE, OTHER]);
    let first_copy_end = base.find(REBASE).expect("first copy") + REBASE.len();
    // Typed at the end of the first copy's line: inside its cell.
    let ops = [EditorOp::Insert {
        offset: first_copy_end - 1,
        text: format!("\n{}", REBASE.trim_end_matches('\n')),
    }];
    let outcome = merge_3way_with_ops(&base, &agent, &editor, &ops);
    assert!(!outcome.fell_back, "op-pinned merge must not refuse");
    assert_eq!(
        outcome.merged_text,
        doc(&[HEAD, REBASE, REBASE, STRUCK, OTHER])
    );
}

/// The op-pinned edit of one copy: the untouched copy keeps the strike in
/// either merge order.
#[test]
fn op_origin_pins_the_untouched_twin_for_an_edit() {
    let base = doc(&[HEAD, REBASE, REBASE, OTHER]);
    let agent = doc(&[HEAD, STRUCK, REBASE, OTHER]);
    let editor = doc(&[HEAD, REBASE, EDITED, OTHER]);
    let second_copy_end = base.rfind(REBASE).expect("second copy") + REBASE.len();
    let ops = [EditorOp::Insert {
        offset: second_copy_end - 1,
        text: " onto main".to_string(),
    }];
    let outcome = merge_3way_with_ops(&base, &agent, &editor, &ops);
    assert!(!outcome.fell_back);
    assert_eq!(outcome.merged_text, doc(&[HEAD, STRUCK, EDITED, OTHER]));
}
