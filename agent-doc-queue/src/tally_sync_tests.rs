//! Tests for `#answeredentry` across readers: a response's tally, the top-level open-work count
//! and the Loose ends digest read one answer, and a tally no response owns is
//! never left frozen.

use super::*;
use crate::loose_ends::{LooseEndGroup, loose_ends};
use crate::queue_tree::{QueueTreeOptions, RowResolution, queue_tree};

/// The live tree (anonymized): three agent action items fixed, and two 🙋
/// follow-up rows whose Remaining entries the operator answered with
/// `~~#justdoit~~`, the first with a `Complete.` response. The operator read
/// it as 5/5 done.
const ANSWERED: &str = include_str!("../tests/fixtures/response_remaining_answered_entries.md");
const ANSWERED_ROOT: &str = "- **Response [#r-ed4566860530]**";

/// The live document after a top-level row was torn in between that tree's
/// Remaining list and its State, and the operator then deleted the tree: the
/// State and Guidance (`🙋 3/5 done, 2 yours.`) stayed under the moved row,
/// beside the two 🙋 follow-up rows.
const DETACHED: &str = include_str!("../tests/fixtures/response_remaining_detached_tally.md");

const VSCODE_ROW: &str = "q-cb3ac1276fd3";
const TITLES_ROW: &str = "q-dade5157fe53";
const DETACHED_STATE: &str = "    - **State:** Waiting on operator input.\n";
const DETACHED_GUIDANCE: &str = "    - **Guidance:** 🙋 3/5 done, 2 yours.";

/// Every sync pass that renders a tally, a count or the digest's source, as
/// preflight and `queue sync` run them.
fn full_sync(document: &str) -> String {
    let synced = crate::response_tree_sync::sync_response_trees_striking(document, &|_| false)
        .map_or_else(|| document.to_string(), |synced| synced.document);
    crate::open_count::reconcile_open_counts(&synced)
        .expect("counts render")
        .unwrap_or(synced)
}

fn loose_end_ids(document: &str) -> Vec<(LooseEndGroup, String)> {
    loose_ends(document)
        .expect("digest renders")
        .into_iter()
        .map(|end| (end.group, end.id))
        .collect()
}

/// The queue row whose text contains `id`.
fn row_line<'a>(document: &'a str, id: &str) -> &'a str {
    document
        .lines()
        .find(|line| line.contains(&format!("[#{id}]")) && !line.contains("Loose"))
        .expect("row")
}

/// The answered rows are done for every reader: the tree resolves them, the
/// digest and the count leave them out, and the full sync finishes the tree
/// with every item done (the operator's `5/5`, then the struck root).
/// Before this the tally said `✅ 5/5 done.` while the digest still
/// listed the VS Code row as a decision and the root counted `(1 decision)`.
#[test]
fn answered_rows_are_done_for_the_tally_the_count_and_the_digest() {
    let tree = queue_tree(ANSWERED, &QueueTreeOptions::default()).expect("tree");
    let resolution = |id: &str| {
        tree.nodes
            .iter()
            .find(|node| node.full_text.contains(&format!("[#{id}]")))
            .expect("row node")
            .resolution
    };
    assert_eq!(resolution(VSCODE_ROW), Some(RowResolution::Answered));
    // The titles row asks to clean up agent-doc's own artifacts, which
    // self-repair already did; that reading comes first.
    assert_eq!(resolution(TITLES_ROW), Some(RowResolution::SelfRepaired));
    let ids = loose_end_ids(ANSWERED);
    assert!(
        !ids.iter()
            .any(|(_, id)| id == VSCODE_ROW || id == TITLES_ROW),
        "{ids:?}"
    );

    let synced = full_sync(ANSWERED);
    let (state, guidance) = state_and_guidance(&synced, ANSWERED_ROOT);
    assert_eq!(state, RESPONSE_STATE_COMPLETE, "{synced}");
    // The five items are done, so the tree is struck from the root down and
    // the struck root joins its tally (`#treefinish`).
    assert_eq!(
        guidance,
        format!("✅ 6/6 done. {NOTHING_REMAINS_ROW_GUIDANCE}"),
        "{synced}"
    );
    let head = synced
        .lines()
        .find(|line| line.contains("agent-doc duplicated my prompt"))
        .expect("root row");
    assert!(!head.contains("decision"), "the count agrees: {head}");
    assert!(loose_end_ids(&synced).is_empty(), "{synced}");
    assert_eq!(full_sync(&synced), synced, "re-sync is a no-op");
}

/// While the agent still works an answer, the row is the agent's: the tally
/// says `⏳` with nothing yours, and neither the digest nor the count lists
/// it as the operator's decision.
#[test]
fn a_live_answer_takes_the_row_off_the_operators_list() {
    let start = ANSWERED
        .find("        - ~~#justdoit~~\n          - **Response")
        .expect("answer");
    let end = ANSWERED[start..]
        .find("      - 🙋 operator: apply edits")
        .map(|at| start + at)
        .expect("next entry");
    let live = format!(
        "{}        - #justdoit\n{}",
        &ANSWERED[..start],
        &ANSWERED[end..]
    );
    let tree = queue_tree(&live, &QueueTreeOptions::default()).expect("tree");
    let node = tree
        .nodes
        .iter()
        .find(|node| node.full_text.contains(&format!("[#{VSCODE_ROW}]")))
        .expect("row node");
    assert!(node.answer_pending);
    assert_eq!(node.resolution, None);

    let synced = full_sync(&live);
    let (_, guidance) = state_and_guidance(&synced, ANSWERED_ROOT);
    assert!(guidance.starts_with("⏳ 4/5 done."), "{guidance}");
    assert!(!guidance.contains("yours"), "{guidance}");
    let ids = loose_end_ids(&synced);
    assert!(!ids.iter().any(|(_, id)| id == VSCODE_ROW), "{ids:?}");
    let head = synced
        .lines()
        .find(|line| line.contains("agent-doc duplicated my prompt"))
        .expect("root row");
    assert!(!head.contains("decision"), "{head}");
    assert_eq!(full_sync(&synced), synced, "re-sync is a no-op");
}

/// The live shape: a frozen `🙋 3/5 done, 2 yours.` no response owns. The
/// sync drops it; the rows beside it stay, and the moved row's count and the
/// digest name the same open work.
#[test]
fn a_tally_no_response_owns_is_dropped_on_sync() {
    assert!(DETACHED.contains(DETACHED_STATE) && DETACHED.contains(DETACHED_GUIDANCE));
    let (dropped, count) =
        drop_detached_response_guidance(DETACHED).expect("the detached pair is dropped");
    assert_eq!(count, 1);
    assert_eq!(
        dropped,
        DETACHED.replacen(DETACHED_STATE, "", 1).replacen(
            &format!(
                "{}\n",
                DETACHED
                    .lines()
                    .find(|line| line.starts_with(DETACHED_GUIDANCE))
                    .expect("guidance")
            ),
            "",
            1
        )
    );
    assert_eq!(drop_detached_response_guidance(&dropped), None);

    let synced = full_sync(DETACHED);
    assert!(!synced.contains(DETACHED_STATE), "{synced}");
    assert!(!synced.contains(DETACHED_GUIDANCE), "{synced}");
    // The rows the frozen tally counted are untouched.
    for id in [VSCODE_ROW, TITLES_ROW] {
        assert!(row_line(&synced, id).starts_with("    - 🙋 "), "{id}");
    }
    // The count on the moved row is the digest's own reading of its tree.
    let ids = loose_end_ids(&synced);
    let decisions = ids
        .iter()
        .filter(|(group, _)| *group == LooseEndGroup::Decide)
        .count();
    assert!(
        ids.contains(&(LooseEndGroup::Decide, VSCODE_ROW.to_string())),
        "{ids:?}"
    );
    let moved = row_line(&synced, "q-e107b04c1cd5");
    assert!(
        moved.contains(&format!(
            "({decisions} decision{})",
            if decisions == 1 { "" } else { "s" }
        )),
        "{moved}\n{ids:?}"
    );
    assert_eq!(full_sync(&synced), synced, "re-sync is a no-op");
}

/// While the response the pair was cut from is still in the document, the
/// pair is left for the moved row to go back out (`#e107moved`).
#[test]
fn a_tally_cut_off_from_a_live_response_is_left_alone() {
    let split = ANSWERED.replacen(
        "    - **State:** Waiting on operator input.\n",
        "- ### a row torn into the response\n    - **State:** Waiting on operator input.\n",
        1,
    );
    assert_ne!(split, ANSWERED);
    assert_eq!(drop_detached_response_guidance(&split), None);
}

/// The State and Guidance values of the response whose row is `marker`.
fn state_and_guidance(document: &str, marker: &str) -> (String, String) {
    let start = document.find(marker).expect("response");
    let indent = document[..start].len() - document[..start].trim_end_matches(' ').len();
    let detail = format!("{}- ", " ".repeat(indent + 2));
    let mut state = String::new();
    let mut guidance = String::new();
    for line in document[start..].lines().skip(1) {
        let line_indent = line.len() - line.trim_start().len();
        if !line.trim().is_empty() && line_indent <= indent {
            break;
        }
        if let Some(rest) = line.strip_prefix(&detail) {
            if let Some(value) = rest.strip_prefix("**State:** ") {
                state = value.to_string();
            } else if let Some(value) = rest.strip_prefix("**Guidance:** ") {
                guidance = value.to_string();
            }
        }
    }
    (state, guidance)
}

/// The live tree (anonymized): a response whose only Remaining entry is an
/// agent item (`⏳ agent: coordinator runs the single make install …`), which
/// the operator answered with `~~#justdoit~~`; its response says
/// `Complete.`. The tree said `⏳ 1/2 done.` (earlier `More work remains.`)
/// although nothing remained.
const AGENT_ENTRY_ANSWERED: &str =
    include_str!("../tests/fixtures/response_remaining_agent_entry_answered.md");

/// An answered agent Remaining entry finishes the tally: the entry turns ✅,
/// the response is `Complete.` with `✅ 2/2 done. 🗑️ Safe to delete this
/// tree.`, the root is struck as deletable, and a second sync is a no-op.
#[test]
fn an_answered_agent_remaining_entry_finishes_the_tally() {
    let (state, guidance) =
        state_and_guidance(AGENT_ENTRY_ANSWERED, "- **Response [#r-6025882e5c0f]**");
    assert_eq!(state, "Follow-up work remains.");
    assert!(guidance.starts_with("⏳ 1/2 done."), "{guidance}");

    let synced = full_sync(AGENT_ENTRY_ANSWERED);
    let (state, guidance) = state_and_guidance(&synced, "- **Response [#r-6025882e5c0f]**");
    assert_eq!(state, "Complete.", "{synced}");
    assert_eq!(
        guidance, "✅ 2/2 done. 🗑\u{fe0f} Safe to delete this tree.",
        "{synced}"
    );
    assert!(
        synced.contains("      - ✅ agent: coordinator runs the single `make install`"),
        "{synced}"
    );
    assert!(
        synced.contains("- ### ~~🗑\u{fe0f} The subitem "),
        "{synced}"
    );
    assert!(
        !synced.lines().any(|line| line
            .trim_start()
            .starts_with("- **Guidance:** More work remains")),
        "{synced}"
    );
    assert_eq!(full_sync(&synced), synced);
}
