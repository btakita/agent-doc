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
const FOLLOW_UP: &str = "  - 🙋 follow up on an action item from `completetasksplans`: ";

/// The struck row's own subtree (its response tree), up to its next sibling.
fn struck_tree(document: &str) -> &str {
    let start = document.find(STRUCK).expect("struck second-level row");
    let rest = &document[start + STRUCK.len()..];
    let end = rest
        .find("\n  - ")
        .map_or(document.len(), |at| start + STRUCK.len() + at + 1);
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
