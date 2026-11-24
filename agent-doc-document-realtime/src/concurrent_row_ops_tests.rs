use super::*;
use agent_doc_merge::crdt::{CrdtDoc, merge_by_component};

const PR: &str = "- ### https://github.com/acme/api/pull/930\n";
const NOTE: &str = "  - %% fix(auth): always issue durable credentials %%\n";
const OTHER: &str = "- ### https://github.com/acme/api/pull/933\n";
const TYPED: &str = "- operator typed a new row\n";
const CLAIMED: &str = "  - 🚧 [#subagent:jwtadopt#1] use stateless JWTs?\n";
const REPORTED: &str = "  - ☑️ [#subagent:jwtadopt#1] use stateless JWTs? [#q-b55e5bde5b7a]\n";
const STRUCK: &str = "  - ~~use stateless JWTs?~~\n";
const STRUCK_WITH_ID: &str = "  - ~~use stateless JWTs? [#q-b55e5bde5b7a]~~\n";
const F1: &str = "  - [#q-b55e5bde5b7a] follow-up: operator: choose one [operator-verify]\n";
const F2: &str = "  - [#q-b55e5bde5b7a] follow-up: operator: hold #930 [operator-verify]\n";
const RESPONSE: &str =
    "    - **Response [#r-ddc5f0eb1a59]**\n      - **State:** Follow-up work remains.\n";

fn doc(rows: &[&str]) -> String {
    format!(
        "## Run <!-- agent:run id=\"main\" subagents -->\n{}<!-- /agent:run -->\n",
        rows.concat()
    )
}

/// Merge `older` and `newer` over `base` after the reconciliation, with
/// either side as `ours`.
fn merged_both_orders(base: &str, older: &str, newer: &str) -> [String; 2] {
    let (base, older) = reconcile_concurrent_queue_row_ops(base, older, newer).map_or_else(
        || (base.to_string(), older.to_string()),
        |ops| (ops.base, ops.older),
    );
    let state = CrdtDoc::from_text(&base).encode_state();
    [
        merge_by_component(Some(&state), &older, newer).unwrap(),
        merge_by_component(Some(&state), newer, &older).unwrap(),
    ]
}

#[test]
fn id_admission_and_followups_racing_a_strike_and_response_attach_fold_into_one_row() {
    let base = doc(&[PR, NOTE, CLAIMED, OTHER]);
    let older = doc(&[PR, NOTE, REPORTED, F1, F2, OTHER]);
    let newer = doc(&[PR, NOTE, STRUCK, RESPONSE, OTHER, TYPED]);
    let ops = reconcile_concurrent_queue_row_ops(&base, &older, &newer).expect("reconciled");
    assert_eq!((ops.folded, ops.ordered), (1, 1), "{ops:?}");
    let expected = doc(&[PR, NOTE, STRUCK_WITH_ID, RESPONSE, F1, F2, OTHER, TYPED]);
    for merged in merged_both_orders(&base, &older, &newer) {
        assert_eq!(merged, expected);
    }
}

/// The strike is the older (retained) write and the id admission with its
/// follow-ups the newer one.
#[test]
fn a_retained_strike_and_attach_composed_under_a_newer_followup_insert_fold_into_one_row() {
    let base = doc(&[PR, NOTE, CLAIMED, OTHER]);
    let older = doc(&[PR, NOTE, STRUCK, RESPONSE, OTHER]);
    let newer = doc(&[PR, NOTE, REPORTED, F1, F2, OTHER, TYPED]);
    let expected = doc(&[PR, NOTE, STRUCK_WITH_ID, RESPONSE, F1, F2, OTHER, TYPED]);
    for merged in merged_both_orders(&base, &older, &newer) {
        assert_eq!(merged, expected);
    }
}

#[test]
fn a_response_tree_and_followups_inserted_at_one_gap_keep_the_tree_under_its_origin() {
    let origin =
        "  - ~~[#q-7f1e992b915b] follow-up: agent: re-pin the SDK PRs [#q-bd5a2af5f518]~~\n";
    let f1 = "  - [#q-bd5a2af5f518] follow-up: agent: re-pin #393 [#q-aa11bb22cc33]\n";
    let base = doc(&[PR, NOTE, origin, OTHER]);
    let with_followups = doc(&[PR, NOTE, origin, f1, F2, OTHER]);
    let with_response = doc(&[PR, NOTE, origin, RESPONSE, OTHER, TYPED]);
    let expected = doc(&[PR, NOTE, origin, RESPONSE, f1, F2, OTHER, TYPED]);
    for (older, newer) in [
        (&with_followups, &with_response),
        (&with_response, &with_followups),
    ] {
        let ops = reconcile_concurrent_queue_row_ops(&base, older, newer).expect("reconciled");
        assert_eq!((ops.folded, ops.ordered), (0, 1), "{ops:?}");
        for merged in merged_both_orders(&base, older, newer) {
            let merged = merged.replace(TYPED, "");
            assert_eq!(merged, expected.replace(TYPED, ""));
        }
    }
}

/// Blocks of equal depth at one gap need no order; nothing is rewritten.
#[test]
fn sibling_blocks_at_one_gap_are_left_to_the_merge() {
    let base = doc(&[PR, NOTE, OTHER]);
    let older = doc(&[PR, NOTE, F1, OTHER]);
    let newer = doc(&[PR, NOTE, F2, OTHER]);
    assert_eq!(
        reconcile_concurrent_queue_row_ops(&base, &older, &newer),
        None
    );
}

/// A row the operator rewrote is a different row, not a projection to fold.
#[test]
fn a_row_whose_work_text_changed_is_not_folded() {
    let base = doc(&[PR, NOTE, CLAIMED, OTHER]);
    let older = doc(&[PR, NOTE, REPORTED, OTHER]);
    let newer = doc(&[PR, NOTE, "  - use stateless JWTs everywhere?\n", OTHER]);
    assert_eq!(
        reconcile_concurrent_queue_row_ops(&base, &older, &newer),
        None
    );
}

/// Two different generated ids are two rows' identities, not one.
#[test]
fn rows_with_two_different_generated_ids_are_not_folded() {
    let base = doc(&[PR, NOTE, CLAIMED, OTHER]);
    let older = doc(&[PR, NOTE, REPORTED, OTHER]);
    let newer = doc(&[
        PR,
        NOTE,
        "  - ~~use stateless JWTs? [#q-000000000000]~~\n",
        OTHER,
    ]);
    assert_eq!(
        reconcile_concurrent_queue_row_ops(&base, &older, &newer),
        None
    );
}

/// An un-strike on one side (an operator re-arm) is never folded into the
/// other side's strike.
#[test]
fn a_struck_base_row_one_side_unstruck_is_not_folded() {
    let base = doc(&[PR, NOTE, STRUCK, OTHER]);
    let older = doc(&[
        PR,
        NOTE,
        "  - ~~use stateless JWTs? [#q-b55e5bde5b7a]~~\n",
        OTHER,
    ]);
    let newer = doc(&[PR, NOTE, "  - use stateless JWTs?\n", OTHER]);
    assert_eq!(
        reconcile_concurrent_queue_row_ops(&base, &older, &newer),
        None
    );
}

/// Components other than the runnable queues are not touched.
#[test]
fn non_queue_components_are_not_reconciled() {
    let wrap = |body: &str| format!("<!-- agent:status -->\n{body}<!-- /agent:status -->\n");
    let base = wrap("- a\n");
    let older = wrap("- a\n  - child\n");
    let newer = wrap("- a\n- sibling\n");
    assert_eq!(
        reconcile_concurrent_queue_row_ops(&base, &older, &newer),
        None
    );
}

#[test]
fn combine_row_edits_projects_the_strike_and_the_id_onto_the_newer_line() {
    assert_eq!(
        combine_row_edits(CLAIMED, REPORTED, STRUCK).as_deref(),
        Some(STRUCK_WITH_ID)
    );
    assert_eq!(
        combine_row_edits(CLAIMED, STRUCK, REPORTED).as_deref(),
        Some(STRUCK_WITH_ID)
    );
    // A heading row keeps its `### `.
    assert_eq!(
        combine_row_edits(
            "- ### 🚧 [#subagent:w#1] ship it\n",
            "- ### ☑️ [#subagent:w#1] ship it [#q-0123456789ab]\n",
            "- ### ~~ship it~~\n",
        )
        .as_deref(),
        Some("- ### ~~ship it [#q-0123456789ab]~~\n")
    );
}
