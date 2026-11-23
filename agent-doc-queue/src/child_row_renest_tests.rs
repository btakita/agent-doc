//! Tests for [`super`] (`#selfrepaircleanup`).

use super::*;

/// The live tree (anonymized): a finished response tree, then the operator's
/// column-0 row quoting two of its lines in one code span, whose second line
/// (6 spaces deep) was answered and struck with its own response beneath it.
const STRUCK_ANSWER: &str = include_str!("../tests/fixtures/child_row_struck_answer_session.md");

/// The live tree (anonymized): two `🙋` follow-up rows, each answered with a
/// nested reply, left at 4 spaces under a column-0 row after the response
/// tree that held them was deleted.
const FOLLOWUPS: &str = include_str!("../tests/fixtures/child_row_followups_session.md");

const OPERATOR_ROW: &str = "- ### `- ### ~~The subitem ";
const ANSWER_LEAD: &str = "- ~~agent: coordinator runs the single `make install` for `80a41b02c4`";

fn queue(body: &str) -> String {
    format!("<!-- agent:run id=\"main\" -->\n{body}<!-- /agent:run -->\n")
}

/// Every line with its leading spaces removed.
fn stripped(text: &str) -> Vec<&str> {
    text.lines()
        .map(|line| line.trim_start_matches(' '))
        .collect()
}

fn line_starting<'a>(document: &'a str, lead: &str) -> &'a str {
    document
        .lines()
        .find(|line| line.trim_start().starts_with(lead))
        .unwrap_or_else(|| panic!("no line starting {lead:?}"))
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

/// The struck answer 6 spaces under its column-0 parent moves to 2, its
/// response with it; only indentation changes, the repair is logged, and a
/// second run is a no-op.
#[test]
fn a_struck_answer_six_spaces_under_a_column_zero_row_moves_to_one_level() {
    let before = line_starting(STRUCK_ANSWER, ANSWER_LEAD);
    assert_eq!(indent(before), 6);
    let (repaired, repairs) = renest_child_rows(STRUCK_ANSWER);
    assert_eq!(repairs.len(), 1, "{repairs:?}");
    assert_eq!(repairs[0].kind, SelfRepairKind::MisnestedChildRow);
    assert!(
        repairs[0]
            .log_fields()
            .starts_with("kind=misnested_child_row detail=\"~~agent: coordinator")
    );

    let at = repaired.find(OPERATOR_ROW).unwrap();
    let after: Vec<&str> = repaired[at..].lines().skip(1).take(6).collect();
    assert!(
        after[0].starts_with(&format!("  {ANSWER_LEAD}")),
        "{}",
        after[0]
    );
    assert_eq!(after[1], "    - **Response [#r-f3f797216300]**");
    assert!(
        after[2].starts_with("      - **Timestamp:** "),
        "{}",
        after[2]
    );
    assert!(
        after[5].starts_with("      - **Guidance:** "),
        "{}",
        after[5]
    );
    assert_eq!(stripped(&repaired), stripped(STRUCK_ANSWER));
    // The finished tree above, and its answered `#justdoit`, did not move.
    assert_eq!(repaired[..at], STRUCK_ANSWER[..at]);
    assert_eq!(renest_child_rows(&repaired), (repaired.clone(), Vec::new()));
}

/// The two follow-up rows 4 spaces under their column-0 parent move to 2,
/// and their answers to 4; text is byte-for-byte unchanged.
#[test]
fn follow_up_rows_one_level_too_deep_move_with_their_answers() {
    let (repaired, repairs) = renest_child_rows(FOLLOWUPS);
    assert_eq!(repairs.len(), 2, "{repairs:?}");
    assert!(
        repairs
            .iter()
            .all(|repair| repair.kind == SelfRepairKind::MisnestedChildRow)
    );
    let vscode = line_starting(
        &repaired,
        "- 🙋 follow up on an action item from queue item “follow up on an action item from queue item 'complete",
    );
    assert_eq!(indent(vscode), 2);
    assert!(
        vscode.ends_with("[operator-verify] [#q-cb3ac1276fd3]"),
        "{vscode}"
    );
    let titles = line_starting(
        &repaired,
        "- 🙋 follow up on an action item from queue item “complete the work",
    );
    assert_eq!(indent(titles), 2);
    assert!(titles.ends_with("[#q-dade5157fe53]"), "{titles}");
    assert!(repaired.contains("\n    - 📥 #justdoit\n"), "{repaired}");
    assert!(
        repaired.contains("\n    - 📥 clean up the titles\n"),
        "{repaired}"
    );
    assert_eq!(stripped(&repaired), stripped(FOLLOWUPS));
    assert_eq!(renest_child_rows(&repaired), (repaired.clone(), Vec::new()));
}

/// Rows one step under their parent are left alone; a deeper row keeps its subtree's relative depth when it moves.
#[test]
fn rows_one_step_under_their_parent_stay_and_subtrees_keep_their_shape() {
    let fine = queue(concat!(
        "- ### root\n",
        "  - child\n",
        "    - grandchild\n",
        "      grandchild continuation\n",
    ));
    assert_eq!(renest_child_rows(&fine), (fine.clone(), Vec::new()));

    let deep = queue(concat!(
        "- ### root\n",
        "  - child\n",
        "        - too deep\n",
        "          - its child\n",
        "            continuation text\n",
        "  - sibling\n",
    ));
    let (repaired, repairs) = renest_child_rows(&deep);
    assert_eq!(repairs.len(), 1);
    assert_eq!(
        repaired,
        queue(concat!(
            "- ### root\n",
            "  - child\n",
            "    - too deep\n",
            "      - its child\n",
            "        continuation text\n",
            "  - sibling\n",
        ))
    );
}

/// Fail closed: tabs, fenced code, and a run that fits the tree above one
/// level below its response row or a Remaining entry (torn off that tree,
/// `#e107moved`) are never moved.
#[test]
fn tabs_fences_and_runs_torn_off_the_tree_above_stay() {
    let tabbed = queue("- ### root\n\t\t- tabbed child\n");
    assert_eq!(renest_child_rows(&tabbed), (tabbed.clone(), Vec::new()));

    let fenced = queue(concat!(
        "- ### root\n",
        "  ```text\n",
        "      - not a row\n",
        "  ```\n",
    ));
    assert_eq!(renest_child_rows(&fenced), (fenced.clone(), Vec::new()));

    let torn_response = queue(concat!(
        "- ### ~~done row~~\n",
        "  - **Response [#r-0123456789ab]**\n",
        "    - **Summary:** Fixed.\n",
        "- ### row torn into the response\n",
        "    - **State:** Complete.\n",
        "    - **Guidance:** ✅ Done.\n",
    ));
    assert_eq!(
        renest_child_rows(&torn_response),
        (torn_response.clone(), Vec::new())
    );

    let torn_entry = queue(concat!(
        "- ### row\n",
        "  - **Response [#r-0123456789ab]**\n",
        "    - **Remaining:**\n",
        "      - ⏳ agent: ship it <!-- agent-doc:response-detail -->\n",
        "- ### row torn under the entry\n",
        "        - ~~#justdoit~~\n",
        "          - **Response [#r-ba9876543210]**\n",
        "            - **State:** Complete.\n",
    ));
    assert_eq!(
        renest_child_rows(&torn_entry),
        (torn_entry.clone(), Vec::new())
    );
}

/// End to end through self-repair and the response-tree sync: the operator's
/// row is queue work of its own (its quoted marker does not make it a
/// response detail), its struck answer sits one level under it with a current
/// Guidance, and a second sync is a no-op.
#[test]
fn full_sync_renests_the_answer_and_its_guidance_follows_its_tree() {
    let full = |document: &str| {
        let repaired = crate::self_repair::self_repair_document_with_provenance(
            document,
            &|_, _| false,
            &|_| true,
            &|_| false,
        )
        .map_or_else(|| document.to_string(), |(repaired, _)| repaired);
        crate::response_tree_sync::sync_response_trees_striking(&repaired, &|_| false)
            .map_or(repaired, |synced| synced.document)
    };
    let tree = crate::queue_tree::queue_tree(STRUCK_ANSWER, &Default::default()).unwrap();
    assert!(
        tree.nodes
            .iter()
            .any(|node| node.depth == 0 && node.text.starts_with("`- ### ~~The subitem")),
        "{}",
        crate::queue_tree::render_text(&tree)
    );
    let synced = full(STRUCK_ANSWER);
    let answer = line_starting(&synced, ANSWER_LEAD);
    assert_eq!(indent(answer), 2);
    let at = synced.find(answer).unwrap();
    assert!(
        synced[at..]
            .contains("\n      - **Guidance:** ✅ Done. 🗑\u{fe0f} Safe to delete this tree.\n"),
        "{synced}"
    );
    assert!(!synced.contains("Its tree was not checked"), "{synced}");
    assert_eq!(full(&synced), synced);
}
