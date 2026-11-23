//! Tests for [`super`] (`#guidancescope`, `#handbackrowstructure`).

use super::*;

/// The live shape, anonymized: a root pull-request row whose second-level
/// row is struck with the worker hand-back copied under it, five open
/// operator follow-up rows beside it, and the hand-back's completion cell
/// still in `agent:exchange`. The struck row's Guidance was written when the
/// row was struck; the operator had since deleted a second-level row.
const SESSION: &str = include_str!("../tests/fixtures/subtree_guidance_session.md");

const ROOT: &str = "- ### https://github.com/sample-org/sample-sdk/pull/368\n";
const STRUCK: &str = "  - ~~do [#completetasksplans]~~\n";
/// [`STRUCK`] once a sync marks it the top of a deletable subtree
/// (`#treetopglyph`).
const STRUCK_CUT: &str = "  - ~~🗑\u{fe0f} do [#completetasksplans]~~\n";
const FOLLOW_UP: &str = "  - 🙋 follow up on an action item from `completetasksplans`: ";

/// The struck row's own subtree (its response tree), up to its next sibling.
fn struck_tree(document: &str) -> &str {
    let (start, row) = [STRUCK, STRUCK_CUT]
        .into_iter()
        .find_map(|row| document.find(row).map(|start| (start, row)))
        .expect("struck second-level row");
    let rest = &document[start + row.len()..];
    let end = rest
        .find("\n  - ")
        .map_or(document.len(), |at| start + row.len() + at + 1);
    &document[start..end]
}

fn guidance(document: &str) -> &str {
    struck_tree(document)
        .lines()
        .find_map(|line| line.trim_start().strip_prefix("- **Guidance:** "))
        .expect("guidance")
}

fn without_followup_rows(document: &str, keep: usize) -> String {
    let mut seen = 0;
    document
        .split_inclusive('\n')
        .filter(|line| {
            if !line.starts_with(FOLLOW_UP) {
                return true;
            }
            seen += 1;
            seen <= keep
        })
        .collect()
}

/// A finished second-level row in an unfinished tree says only that it is
/// done (`#treefinish`): no "Safe to delete" as if the item were done, and no
/// "keep the rest of the tree" sentence on every child.
#[test]
fn a_done_second_level_row_says_its_root_item_is_still_open() {
    assert!(SESSION.contains(ROOT));
    assert!(guidance(SESSION).starts_with("Safe to delete this completed row;"));

    let (synced, rows) = reconcile_tree_scoped_guidance(SESSION).expect("guidance synced");
    assert_eq!(rows, 1);
    assert_eq!(guidance(&synced), "✅ Done.");
    assert!(
        reconcile_tree_scoped_guidance(&synced).is_none(),
        "a second pass changes nothing"
    );
    // Only the Guidance value changed.
    assert_eq!(
        synced.replace(guidance(&synced), ""),
        SESSION.replace(guidance(SESSION), "")
    );
}

/// The operator deletes second-level rows: the struck row's Guidance follows
/// the tree on the next sync, down to the whole tree being deletable.
#[test]
fn deleting_second_level_rows_resyncs_the_guidance() {
    let one_left = without_followup_rows(SESSION, 1);
    let (synced, _) = reconcile_tree_scoped_guidance(&one_left).expect("guidance synced");
    assert_eq!(guidance(&synced), "✅ Done.");

    let none_left = without_followup_rows(&synced, 0);
    let (closed, _) = reconcile_tree_scoped_guidance(&none_left).expect("guidance synced");
    assert_eq!(guidance(&closed), "✅ Done. 🗑️ Safe to delete this tree.");
    assert!(reconcile_tree_scoped_guidance(&closed).is_none());

    // A row added back reopens the tree.
    let reopened = closed.replace(
        "- ### Deploy the README",
        "  - do [#merge368]\n- ### Deploy the README",
    );
    let (reopened, _) = reconcile_tree_scoped_guidance(&reopened).expect("guidance synced");
    assert_eq!(guidance(&reopened), "✅ Done.");
}

/// The copied hand-back sections leave the row; one Details row points at
/// the hand-back in agent:exchange. Summary, State and the other rows stay.
#[test]
fn copied_handback_sections_move_behind_a_details_row() {
    let before = struck_tree(SESSION);
    for label in [
        "**What changed:**",
        "**Verification:**",
        "**Action items:**",
        "**Notes:**",
        "**Next actions:**",
    ] {
        assert!(before.contains(label), "fixture copies {label}");
    }

    let (slimmed, rows) = slim_handback_response_details(SESSION).expect("tree slimmed");
    assert_eq!(rows, 1);
    let tree = struck_tree(&slimmed);
    for label in [
        "**What changed:**",
        "**Verification:**",
        "**Action items:**",
        "**Notes:**",
        "**Next actions:**",
        "<!-- agent-doc:response-detail -->",
    ] {
        assert!(!tree.contains(label), "{label} left the row:\n{tree}");
    }
    assert!(
        tree.contains(
            "      - **Details:** What changed, Verification, Notes and Next actions are in the `subagent:more368` hand-back in agent:exchange. What changed names `/work/sample-org/tasks/plans/sdk-cli-public-shadow-builds.md`.\n"
        ),
        "{tree}"
    );
    let summary = |document: &str| {
        struck_tree(document)
            .lines()
            .find(|line| line.contains("**Summary:**"))
            .map(str::to_string)
    };
    assert_eq!(
        summary(&slimmed),
        summary(SESSION),
        "the Summary is evidence"
    );
    assert!(tree.contains("      - **State:** Complete.\n"), "{tree}");
    assert!(
        guidance(&slimmed).ends_with(&format!(" {FOLLOW_UP_MOVED_GUIDANCE_NOTE}")),
        "the removed action items moved to their follow-up rows: {}",
        guidance(&slimmed)
    );
    assert!(
        tree.lines().count() <= 7,
        "the tree is a few lines, not {}:\n{tree}",
        tree.lines().count()
    );
    // Everything outside the struck row's response tree is unchanged.
    let outside = |document: &str| document.replace(struck_tree(document), "");
    assert_eq!(outside(&slimmed), outside(SESSION));
    assert!(slim_handback_response_details(&slimmed).is_none());
}

/// An operator edit inside a copied section keeps that section on the row.
#[test]
fn an_edited_section_stays() {
    // Edit the row's copy, not the hand-back in agent:exchange.
    let queue_at = SESSION.find("<!-- agent:run").unwrap();
    let edited = format!(
        "{}{}",
        &SESSION[..queue_at],
        SESSION[queue_at..].replacen(
            "No changes to any public repo or PR",
            "No changes (operator: check this) to any public repo or PR",
            1,
        )
    );
    let (slimmed, _) = slim_handback_response_details(&edited).expect("other sections slim");
    let tree = struck_tree(&slimmed);
    assert!(tree.contains("**What changed:**"), "{tree}");
    assert!(tree.contains("operator: check this"), "{tree}");
    assert!(!tree.contains("**Verification:**"), "{tree}");
    assert!(
        tree.contains("- **Details:** Verification, Notes and Next actions are in"),
        "{tree}"
    );
}

/// No completion cell in the document: nothing points at, so nothing moves.
#[test]
fn without_the_completion_cell_the_copy_stays() {
    let start = SESSION.find("### Re: Subagent completed").unwrap();
    let end = SESSION.find("<!-- /agent:exchange -->").unwrap();
    let no_cell = format!("{}{}", &SESSION[..start], &SESSION[end..]);
    assert!(slim_handback_response_details(&no_cell).is_none());
}

/// Retiring the completion cell of a row whose tree points at it keeps the
/// full hand-back in agent:exchange as a plain record; without a pointer the
/// cell is deleted as before. Either way it is no longer completion evidence.
#[test]
fn retiring_a_pointed_cell_keeps_the_handback_record() {
    let heads = vec!["do [#completetasksplans]".to_string()];
    let (slimmed, _) = slim_handback_response_details(SESSION).unwrap();
    let (retired, keys) =
        crate::queue_consume::retire_consumed_completion_feedback(&slimmed, &heads);
    assert_eq!(keys.len(), 1);
    assert!(agent_doc_document::subagent_completion::completion_evidence(&retired).is_empty());
    assert!(
        retired.contains(
            "### Re: Subagent hand-back: `subagent:more368` for queue item “do [#completetasksplans]”\n> **Subagent response:**\n"
        ),
        "{retired}"
    );
    assert!(retired.contains("> **What changed**"), "{retired}");

    let (deleted, keys) =
        crate::queue_consume::retire_consumed_completion_feedback(SESSION, &heads);
    assert_eq!(keys.len(), 1);
    assert!(!deleted.contains("Subagent hand-back"), "{deleted}");
    assert!(!deleted.contains("> **What changed**"), "{deleted}");
}

/// Both passes as one successor, idempotent.
#[test]
fn sync_response_trees_is_one_idempotent_successor() {
    let (synced, slimmed, guided) = sync_response_trees(SESSION).expect("synced");
    assert_eq!((slimmed, guided), (1, 1));
    assert_eq!(
        guidance(&synced),
        format!("✅ Done. {FOLLOW_UP_MOVED_GUIDANCE_NOTE}")
    );
    assert!(sync_response_trees(&synced).is_none());
}

#[test]
fn summary_lead_keeps_whole_sentences() {
    assert_eq!(
        summary_lead("Done.\n\nMore detail."),
        Some(("Done.".to_string(), true))
    );
    assert_eq!(summary_lead("- a list\n- of items"), None);
    let long = format!(
        "{} Second sentence `a. B` here. {}",
        "A".repeat(300),
        "C".repeat(200)
    );
    let (lead, trimmed) = summary_lead(&long).unwrap();
    assert!(trimmed);
    assert!(lead.ends_with("here."), "{lead}");
}

/// The live tree behind the struck-row Guidance and the backlink re-nesting
/// (anonymized): the operator answered a Next actions entry with `Do the next
/// actions.`, asked under it what had become of that row, and both rows were
/// struck with nothing beneath them.
const STRUCK_COVER_SESSION: &str = include_str!("../tests/fixtures/closed_by_nest_session.md");

/// The struck `Do the next actions.` answer, the top of a deletable subtree
/// under a kept root (`#treetopglyph`).
const DO_NEXT_CUT: &str = "        - ~~🗑\u{fe0f} Do the next actions.~~\n";
const STATUS_QUESTION: &str = "          - ~~What is the status of the parent `        - ~~Do the next actions.~~`? It is struck but there's no response structure.~~\n";

/// The fixture with its last operator row turned into real open operator
/// work (not an ask to clean up an agent-doc artifact, which self-repair
/// resolves), so the tree stays open.
fn open_struck_cover_session() -> String {
    let session = crate::self_repair::renest_closed_by_rows(STRUCK_COVER_SESSION).0;
    let start = session.find("    - 🙋 follow up on an action item from queue item “follow up on an action item from queue item 'follow up on an action item from qu…”: operator: in the editor").unwrap();
    let end = start + session[start..].find('\n').unwrap() + 1;
    let mut open = session.clone();
    open.replace_range(
        start..end,
        "    - 🙋 operator: confirm the release notes read well. [operator-verify] [#q-6608c8ce85b2]\n",
    );
    open
}

/// Every operator line of `before` (not generated response structure) is
/// still in `after`, in order, byte for byte.
fn keeps_every_operator_line(before: &str, after: &str) -> bool {
    let operator = |line: &&str| {
        !line
            .trim_start()
            .strip_prefix("- ")
            .is_some_and(agent_doc_markdown_ast::overlay::is_generated_response_structure_row)
    };
    // `#treetopglyph`: a row's 🗑️/🔒 glyph is binary projection, not
    // operator text.
    let unglyphed = |line: &str| {
        line.replace("🗑\u{fe0f} ", "").replace(
            &format!("{} ", agent_doc_element_queue::TreeIndicator::Keep.glyph()),
            "",
        )
    };
    let mut rest = after.lines().filter(operator).map(unglyphed);
    before
        .lines()
        .filter(operator)
        .all(|line| rest.any(|kept| kept == unglyphed(line)))
}

/// `#treefinish`: the struck `Do the next actions.` answer under a Next
/// actions entry gets one generated `✅ Done.` Guidance as its first child;
/// the struck question under it is covered by that line and gets none. The
/// response's tally counts both rows as done. Operator text is unchanged and
/// a second sync is a no-op.
#[test]
fn struck_answer_under_a_detail_row_gets_a_done_guidance() {
    let session = open_struck_cover_session();
    let (synced, _, _) = sync_response_trees(&session).unwrap();
    let guided = format!("{DO_NEXT_CUT}          - **Guidance:** ✅ Done.\n{STATUS_QUESTION}");
    assert!(synced.contains(&guided), "{synced}");
    assert_eq!(
        synced.matches("**Guidance:** ✅ Done.\n").count(),
        2,
        "{synced}"
    );
    assert!(keeps_every_operator_line(&session, &synced), "{synced}");
    // The root response still waits on the one open operator row, and counts
    // the struck rows as done.
    assert!(
        synced.contains("    - **State:** Waiting on operator input.\n"),
        "{synced}"
    );
    assert!(sync_response_trees(&synced).is_none(), "{synced}");

    // The operator finishes the last row: the tree is finished, and the
    // struck answer's Guidance says so.
    let finished = synced.replace(
        "    - 🙋 operator: confirm the release notes read well. [operator-verify] [#q-6608c8ce85b2]\n",
        "    - ~~operator: confirm the release notes read well. [operator-verify] [#q-6608c8ce85b2]~~\n",
    );
    let (done, _, _) = sync_response_trees(&finished).unwrap();
    assert!(
        done.contains(&format!(
            "{DO_NEXT_CUT}          - **Guidance:** ✅ Done. 🗑️ Safe to delete this tree.\n{STATUS_QUESTION}"
        )),
        "{done}"
    );
    assert!(sync_response_trees(&done).is_none(), "{done}");
}

/// `#treefinish`: in a finished tree the root's indicator covers every
/// struck row, so no per-row line is added; the root says the tree can go.
#[test]
fn struck_rows_in_a_finished_tree_are_covered_by_the_root() {
    let session = crate::self_repair::renest_closed_by_rows(STRUCK_COVER_SESSION).0;
    let (synced, _, _) = sync_response_trees(&session).unwrap();
    // The plain sync never strikes, so the live root is kept and the struck
    // subtree under it is where to cut (`#treetopglyph`).
    assert!(
        synced.contains(&format!("{DO_NEXT_CUT}{STATUS_QUESTION}")),
        "{synced}"
    );
    assert!(
        synced.contains("    - **Guidance:** ✅ 6/6 done. 🗑️ Safe to delete this tree.\n"),
        "{synced}"
    );
    assert!(sync_response_trees(&synced).is_none(), "{synced}");
}

/// `#treefinish`: an operator's own Guidance under a struck row, and a row
/// with its own response, are left alone; a struck row outside any response
/// is not this pass's.
#[test]
fn bare_struck_guidance_leaves_covered_and_unrelated_rows() {
    let document = concat!(
        "<!-- agent:run id=\"main\" subagents -->\n",
        "- ### root\n",
        "  - **Response [#r-2da6da4a7fd8]**\n",
        "    - **Next actions:** <!-- agent-doc:response-detail -->\n",
        "      - Ship it. <!-- agent-doc:response-detail -->\n",
        "        - ~~shipped~~\n",
        "          - **Guidance:** my own note\n",
        "        - ~~answered~~\n",
        "          - **Response [#r-37bf1620fdbd]**\n",
        "            - **State:** Complete.\n",
        "    - **State:** Complete.\n",
        "    - **Guidance:** operator wording\n",
        "  - open work\n",
        "  - ~~outside~~\n",
        "<!-- /agent:run -->\n",
    );
    assert!(guide_bare_struck_rows(document).is_none());
}
