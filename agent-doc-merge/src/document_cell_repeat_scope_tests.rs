//! `#e107moved`: byte-identical nested rows are matched by the row they hang
//! under, so removing one copy never moves another row.

use crate::crdt::{CrdtDoc, merge_by_component};

const DUP_TREE: &str = concat!(
    "- ### agent-doc duplicated my prompt above this prompt.\n",
    "  - **Response [#r-ed4566860530]**\n",
    "    - **Summary:** Fixed and pushed.\n",
    "    - **Remaining:**\n",
    "      - 🙋 operator: decide whether to refresh the VS Code package <!-- agent-doc:response-detail -->\n",
    "        - ~~#justdoit~~\n",
    "          - **Response [#r-4fa4a87420e5]**\n",
    "            - **Summary:** Already fixed on main.\n",
    "            - **State:** Complete.\n",
    "      - 🙋 operator: apply edits 1–3 above <!-- agent-doc:response-detail -->\n",
    "        - ~~#justdoit~~\n",
    "    - **State:** Waiting on operator input.\n",
    "    - **Guidance:** 🙋 3/5 done, 2 yours.\n",
    "    - 🙋 follow up: operator: decide whether to refresh the VS Code package. [operator-verify] [#q-cb3ac1276fd3]\n",
    "    - 🙋 follow up: operator: apply edits 1–3 above. [operator-verify] [#q-dade5157fe53]\n",
);
const TITLES: &str = "- ### 🙋 follow up: operator: decide whether to clean those titles up by hand. [operator-verify] [#q-8eeadf7937c8]\n";
const TITLES_WORKER: &str = "  - 🚧 [#subagent:titlejust#1] #justdoit\n";
const CLEANUP: &str = concat!(
    "- ### ~~follow up: operator: delete the extra struck copies. [operator-verify] [#q-e107b04c1cd5]~~\n",
    "  - ~~#justdoit~~\n",
);
const STOPACK: &str = concat!(
    "- ### follow up: agent: stop `queue consume --ack-text` from re-attaching a response tree.\n",
    "  - **Response [#r-2da6da4a7fd8]**\n",
    "    - **Summary:** Fix pushed.\n",
);
const NEW_ROWS: &str = concat!(
    "- `    - **Guidance:** 🙋 3/5 done, 2 yours.` should have been updated.\n",
    "- `        - ~~#justdoit~~` under `apply edits 1–3` should have had a response.\n",
);

fn doc(rows: &[&str]) -> String {
    format!(
        "## Run <!-- agent:run id=\"main\" subagents -->\n{}<!-- /agent:run -->\n",
        rows.concat()
    )
}

fn merged_both_ways(base: &str, a: &str, b: &str) -> [String; 2] {
    let state = CrdtDoc::from_text(base).encode_state();
    [
        merge_by_component(Some(&state), a, b).expect("merge a over b"),
        merge_by_component(Some(&state), b, a).expect("merge b over a"),
    ]
}

/// The 2026-11-22 cut: an agent write drops the struck cleanup row (and its
/// `~~#justdoit~~` answer) while the operator types two new rows below the
/// tree above. Matched by text alone, the removed answer was read as the
/// first `~~#justdoit~~` (under a Remaining entry): the struck head survived
/// as the parent of the second one, between `**Remaining:**` and
/// `**State:**`, and the answer under the Remaining entry was doubled or its
/// response tree lost. Every row now stays where it was.
#[test]
fn removing_a_repeated_answer_never_moves_its_parent_into_another_tree() {
    let base = doc(&[DUP_TREE, TITLES, TITLES_WORKER, CLEANUP, STOPACK]);
    let agent = doc(&[DUP_TREE, TITLES, TITLES_WORKER, STOPACK]);
    let editor = doc(&[DUP_TREE, NEW_ROWS, TITLES, TITLES_WORKER, CLEANUP, STOPACK]);
    let expected = doc(&[DUP_TREE, NEW_ROWS, TITLES, TITLES_WORKER, STOPACK]);
    for merged in merged_both_ways(&base, &agent, &editor) {
        assert_eq!(merged, expected);
    }
}

/// The operator deletes the struck cleanup row while an agent write changes
/// the worker row under the row above it. The deletion stays a deletion.
#[test]
fn an_operator_deleting_a_repeated_answer_keeps_every_other_tree_whole() {
    let base = doc(&[DUP_TREE, TITLES, TITLES_WORKER, CLEANUP, STOPACK]);
    let agent = doc(&[DUP_TREE, TITLES, "  - ~~#justdoit~~\n", CLEANUP, STOPACK]);
    let editor = doc(&[DUP_TREE, NEW_ROWS, TITLES, TITLES_WORKER, STOPACK]);
    let expected = doc(&[DUP_TREE, NEW_ROWS, TITLES, "  - ~~#justdoit~~\n", STOPACK]);
    for merged in merged_both_ways(&base, &agent, &editor) {
        assert_eq!(merged, expected);
    }
}
