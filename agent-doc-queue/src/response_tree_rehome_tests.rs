use super::*;

fn doc(queue: &str) -> String {
    format!(
        "---\nagent_doc_format: template\n---\n\n## Run <!-- agent:run id=\"main\" subagents -->\n{queue}<!-- /agent:run -->\n\n## Exchange <!-- agent:exchange -->\n<!-- /agent:exchange -->\n"
    )
}

const CLEANUP: &str = "follow up on an action item from queue item “cleanup”: operator: delete the extra struck copies. [operator-verify] [#q-e107b04c1cd5]";

/// The tree above, cut down from the 2026-11-22 incident: a response whose
/// `**Remaining:**` list has answered entries, then its State, Guidance and
/// follow-up rows.
fn tree_head() -> String {
    concat!(
        "- ### agent-doc duplicated my prompt above this prompt.\n",
        "  - **Response [#r-ed4566860530]**\n",
        "    - **Summary:** Fixed and pushed.\n",
        "    - **Remaining:**\n",
        "      - 🙋 operator: decide whether to refresh the VS Code package <!-- agent-doc:response-detail -->\n",
        "        - ~~#justdoit~~\n",
        "      - 🙋 operator: apply edits 1–3 above <!-- agent-doc:response-detail -->\n",
        "        - ~~#justdoit~~\n",
    )
    .to_string()
}

fn tree_tail() -> String {
    concat!(
        "    - **State:** Waiting on operator input.\n",
        "    - **Guidance:** 🙋 3/5 done, 2 yours.\n",
        "    - 🙋 follow up: operator: decide whether to refresh the VS Code package. [operator-verify] [#q-cb3ac1276fd3]\n",
        "    - 🙋 follow up: operator: apply edits 1–3 above. [operator-verify] [#q-dade5157fe53]\n",
    )
    .to_string()
}

const NEXT: &str = "- ### follow up: agent: stop `queue consume --ack-text` from re-attaching.\n";

fn proven(work: &str) -> bool {
    work == CLEANUP
}

fn unproven(_: &str) -> bool {
    false
}

/// `#e107moved`: the live 2026-11-22 shape. The struck cleanup row sat after
/// the tree; a torn merge put it, un-struck and `🙋`, between the Remaining
/// list and the State row, so the tree's own State, Guidance and follow-up
/// rows hung under it. Self-repair moves it back below the tree and restores
/// the strike the committed document proves, keeping its text.
#[test]
fn a_top_level_row_inside_a_response_tree_moves_back_below_it_struck() {
    let split = doc(&format!(
        "{}- ### 🙋 {CLEANUP}\n{}{NEXT}",
        tree_head(),
        tree_tail()
    ));
    let (repaired, repairs) = rehome_rows_inside_response_trees(&split, &proven);
    assert_eq!(
        repaired,
        doc(&format!(
            "{}{}- ### ~~{CLEANUP}~~\n{NEXT}",
            tree_head(),
            tree_tail()
        ))
    );
    assert_eq!(repairs.len(), 1);
    assert_eq!(repairs[0].kind, SelfRepairKind::RowInsideResponseTree);
    assert_eq!(repairs[0].detail, CLEANUP);
    let (again, none) = rehome_rows_inside_response_trees(&repaired, &proven);
    assert_eq!(again, repaired, "a second run changes nothing");
    assert!(none.is_empty());
}

/// Without provenance the row moves but keeps its live text, and a row that
/// was still struck when it landed in the tree moves back struck.
#[test]
fn unproven_rows_move_without_a_strike_and_struck_rows_stay_struck() {
    let live = doc(&format!(
        "{}- ### 🙋 {CLEANUP}\n{}{NEXT}",
        tree_head(),
        tree_tail()
    ));
    let (repaired, _) = rehome_rows_inside_response_trees(&live, &unproven);
    assert_eq!(
        repaired,
        doc(&format!(
            "{}{}- ### 🙋 {CLEANUP}\n{NEXT}",
            tree_head(),
            tree_tail()
        ))
    );

    let struck = doc(&format!(
        "{}- ### ~~{CLEANUP}~~\n{}{NEXT}",
        tree_head(),
        tree_tail()
    ));
    let (repaired, _) = rehome_rows_inside_response_trees(&struck, &unproven);
    assert_eq!(
        repaired,
        doc(&format!(
            "{}{}- ### ~~{CLEANUP}~~\n{NEXT}",
            tree_head(),
            tree_tail()
        ))
    );
}

/// The row's own live children (after the split-off run) keep it live even
/// with provenance: restoring the strike would close open work.
#[test]
fn a_moved_row_with_a_live_child_keeps_its_text() {
    let split = doc(&format!(
        "{}- ### 🙋 {CLEANUP}\n{}  - do this next\n{NEXT}",
        tree_head(),
        tree_tail()
    ));
    let (repaired, _) = rehome_rows_inside_response_trees(&split, &proven);
    assert_eq!(
        repaired,
        doc(&format!(
            "{}{}- ### 🙋 {CLEANUP}\n  - do this next\n{NEXT}",
            tree_head(),
            tree_tail()
        ))
    );
}

/// Nothing proves where the rows belong once the tree above is gone, and an
/// operator row under a row's own child is not response structure: neither
/// shape moves.
#[test]
fn rows_without_a_proven_owning_response_stay() {
    let orphaned = doc(&format!(
        "- ### 🙋 [#q-0e29d5fdf1c4] decide\n  - ~~restarted~~\n- ### 🙋 {CLEANUP}\n{}{NEXT}",
        tree_tail()
    ));
    let (repaired, repairs) = rehome_rows_inside_response_trees(&orphaned, &proven);
    assert_eq!(repaired, orphaned);
    assert!(repairs.is_empty());

    let ordinary = doc(&format!(
        "{}{}- ### 🙋 {CLEANUP}\n  - ~~#justdoit~~\n{NEXT}",
        tree_head(),
        tree_tail()
    ));
    let (repaired, repairs) = rehome_rows_inside_response_trees(&ordinary, &proven);
    assert_eq!(repaired, ordinary);
    assert!(repairs.is_empty());
}

/// Provenance reads a committed copy: the row struck at column 0 outside a
/// response tree proves the strike; the same row inside one does not.
#[test]
fn provenance_needs_the_row_struck_outside_a_response_tree() {
    let committed = doc(&format!(
        "{}{}- ### ~~{CLEANUP}~~\n  - ~~#justdoit~~\n{NEXT}",
        tree_head(),
        tree_tail()
    ));
    assert!(top_level_row_struck_in(&committed, CLEANUP));
    let split = doc(&format!(
        "{}- ### ~~{CLEANUP}~~\n{}{NEXT}",
        tree_head(),
        tree_tail()
    ));
    assert!(!top_level_row_struck_in(&split, CLEANUP));
    let live = doc(&format!(
        "{}{}- ### 🙋 {CLEANUP}\n{NEXT}",
        tree_head(),
        tree_tail()
    ));
    assert!(!top_level_row_struck_in(&live, CLEANUP));
}

/// `#e107moved`: until self-repair moves it, a split-off response's rows are
/// not the column-0 row's leaves. The lazy structural projection neither
/// reopens the struck row over them (the live `🙋` follow-up rows) nor closes
/// the tree above as finished.
#[test]
fn split_off_response_rows_never_reopen_the_row_above_them() {
    let body = format!("{}- ### ~~{CLEANUP}~~\n{}{NEXT}", tree_head(), tree_tail());
    let projected = crate::document_queue::project_prompt_runtime_projections(&body, &[])
        .unwrap()
        .map_or(body.clone(), |(projected, _)| projected);
    assert!(
        projected.contains(&format!("- ### ~~{CLEANUP}~~\n")),
        "the struck row stays struck: {projected}"
    );
    assert!(
        projected.starts_with("- ### agent-doc duplicated my prompt above this prompt.\n"),
        "the tree above stays live: {projected}"
    );
}
