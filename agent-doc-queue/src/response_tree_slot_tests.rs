//! Tests for [`super`] (`#selfrepaircleanup`).

use super::*;

/// The live tree (anonymized): a PR row whose summary tree lost its own row
/// (`  - ~~squash and merge~~`), so it sits 4 spaces under the column-0 row,
/// with the next PR row glued onto its Guidance line.
const LIVE: &str = include_str!("../tests/fixtures/response_tree_slot_session.md");

const SUMMARY: &str = "Squash-merged #375 into dev as e5e1c10ad with the PR title and description.";

fn queue(body: &str) -> String {
    format!("<!-- agent:run id=\"main\" -->\n{body}<!-- /agent:run -->\n")
}

fn kinds(repairs: &[SelfRepair]) -> Vec<SelfRepairKind> {
    repairs.iter().map(|repair| repair.kind).collect()
}

/// Every non-whitespace character, in order: a repair that only moves line
/// breaks and leading spaces keeps it.
fn glyphs(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

fn self_repair(document: &str) -> Option<(String, Vec<SelfRepair>)> {
    crate::self_repair::self_repair_document(document, &|_, _| false, &|_| false)
}

/// The live shape: the glued PR row gets its line back, and the response
/// tree moves up one level under its own PR row. Nothing else changes, both
/// repairs are logged with a kind, and a second sync is a no-op.
#[test]
fn the_live_tree_regains_its_glued_row_and_sits_one_level_below_its_row() {
    let expected = LIVE
        .replace(
            "Safe to delete this tree.- ### 🔒",
            "Safe to delete this tree.\n- ### 🔒",
        )
        .replace(
            "    - **Response [#r-1634a6237f79]**\n",
            "  - **Response [#r-1634a6237f79]**\n",
        )
        .replace("      - **Timestamp:** ", "    - **Timestamp:** ")
        .replace("      - **Summary:** Squash", "    - **Summary:** Squash")
        .replace("      - **State:** ", "    - **State:** ")
        .replace("      - **Guidance:** ", "    - **Guidance:** ");
    let (repaired, repairs) = slot_response_trees(LIVE);
    assert_eq!(repaired, expected);
    assert_eq!(
        kinds(&repairs),
        vec![
            SelfRepairKind::GluedGuidanceRow,
            SelfRepairKind::ResponseTreeSlot
        ]
    );
    assert_eq!(
        repairs[0].log_fields(),
        "kind=glued_guidance_row detail=\"### 🔒 https://github.com/sample-org/sample-sdk/pull/377\""
    );
    assert_eq!(
        repairs[1].log_fields(),
        "kind=response_tree_slot detail=\"**Response [#r-1634a6237f79]**\""
    );
    assert_eq!(glyphs(&repaired), glyphs(LIVE));
    assert_eq!(
        slot_response_trees(&repaired),
        (repaired.clone(), Vec::new())
    );

    // The whole sync chain lands on the same document, then holds still.
    let (synced, repairs) = self_repair(LIVE).expect("the live tree is repaired");
    assert_eq!(synced, expected);
    assert_eq!(
        kinds(&repairs),
        vec![
            SelfRepairKind::GluedGuidanceRow,
            SelfRepairKind::ResponseTreeSlot
        ]
    );
    assert!(self_repair(&synced).is_none());
}

/// An over-indented tree under a row with its own comment and child rows
/// moves after them, one level below the row, its own lines shifted alike.
#[test]
fn an_over_indented_tree_moves_after_the_rows_comment_and_child_rows() {
    let document = queue(concat!(
        "- ### https://example.test/pull/2\n",
        "    - **Response [#r-0123456789ab]**\n",
        "      - **Summary:** Merged.\n",
        "        continuation of the summary\n",
        "      - **State:** Complete.\n",
        "  - %% docs: the title %%\n",
        "  - 📥 #rebase\n",
        "    - an operator note under it\n",
        "- ### https://example.test/pull/3\n",
    ));
    let (repaired, repairs) = slot_response_trees(&document);
    assert_eq!(
        repaired,
        queue(concat!(
            "- ### https://example.test/pull/2\n",
            "  - %% docs: the title %%\n",
            "  - 📥 #rebase\n",
            "    - an operator note under it\n",
            "  - **Response [#r-0123456789ab]**\n",
            "    - **Summary:** Merged.\n",
            "      continuation of the summary\n",
            "    - **State:** Complete.\n",
            "- ### https://example.test/pull/3\n",
        ))
    );
    assert_eq!(kinds(&repairs), vec![SelfRepairKind::ResponseTreeSlot]);
    assert_eq!(
        slot_response_trees(&repaired),
        (repaired.clone(), Vec::new())
    );
    let (synced, _) = self_repair(&document).unwrap();
    assert_eq!(synced, repaired);
    assert!(self_repair(&synced).is_none());
}

/// A tree at the right depth moves after the row's comment rows, and only
/// those: a later child row may be a reply to the response, so it stays.
#[test]
fn a_tree_before_the_rows_comment_moves_after_it_and_replies_stay() {
    let document = queue(concat!(
        "- ### ~~https://example.test/pull/4~~\n",
        "  - **Response [#r-aaaaaaaaaaaa]**\n",
        "    - **Summary:** Merged.\n",
        "    - **State:** Complete.\n",
        "  - %% fix: the title %%\n",
        "  - %% merged %%\n",
        "  - 📥 is the tag out too?\n",
    ));
    let (repaired, repairs) = slot_response_trees(&document);
    assert_eq!(
        repaired,
        queue(concat!(
            "- ### ~~https://example.test/pull/4~~\n",
            "  - %% fix: the title %%\n",
            "  - %% merged %%\n",
            "  - **Response [#r-aaaaaaaaaaaa]**\n",
            "    - **Summary:** Merged.\n",
            "    - **State:** Complete.\n",
            "  - 📥 is the tag out too?\n",
        ))
    );
    assert_eq!(kinds(&repairs), vec![SelfRepairKind::ResponseTreeSlot]);
    assert_eq!(
        slot_response_trees(&repaired),
        (repaired.clone(), Vec::new())
    );

    let replied = queue(concat!(
        "- ### ~~https://example.test/pull/5~~\n",
        "  - %% fix: the title %%\n",
        "  - **Response [#r-bbbbbbbbbbbb]**\n",
        "    - **Summary:** Merged.\n",
        "  - 📥 is the tag out too?\n",
        "  - %% a note after the reply %%\n",
    ));
    assert_eq!(slot_response_trees(&replied), (replied.clone(), Vec::new()));
}

/// Fail closed: a response under generated structure, a tree that may have
/// been torn off the tree above, tabs, fenced code, an operator's Guidance,
/// and a Guidance whose end cannot be proven are left alone.
#[test]
fn ambiguous_shapes_are_left_alone() {
    let under_structure = queue(concat!(
        "- ### ~~row~~\n",
        "  - **Response [#r-cccccccccccc]**\n",
        "    - **Guidance:** ✅ Done.\n",
        "        - **Response [#r-dddddddddddd]**\n",
        "          - **Summary:** copy\n",
    ));
    assert_eq!(
        slot_response_trees(&under_structure),
        (under_structure.clone(), Vec::new())
    );

    let torn = queue(concat!(
        "- ### ~~first~~\n",
        "  - **Response [#r-eeeeeeeeeeee]**\n",
        "- ### split\n",
        "    - **Response [#r-ffffffffffff]**\n",
        "      - **Summary:** belongs above\n",
    ));
    assert_eq!(slot_response_trees(&torn), (torn.clone(), Vec::new()));

    let tabbed = queue("- ### row\n\t\t- **Response [#r-111111111111]**\n  - %% c %%\n");
    assert_eq!(slot_response_trees(&tabbed), (tabbed.clone(), Vec::new()));

    let fenced = queue(concat!(
        "- ### row\n",
        "    - **Response [#r-222222222222]**\n",
        "      ```\n",
        "      - **Guidance:** ✅ Done.- ### glued in code\n",
        "      ```\n",
    ));
    assert_eq!(slot_response_trees(&fenced), (fenced.clone(), Vec::new()));

    let operator_guidance = queue(concat!(
        "- ### ~~row~~\n",
        "  - **Response [#r-333333333333]**\n",
        "    - **Guidance:** keep it- ### not a split\n",
        "    - **Guidance:** 🔒 Keep: 1 open — Done.- ### also not a split\n",
    ));
    assert_eq!(
        slot_response_trees(&operator_guidance),
        (operator_guidance.clone(), Vec::new())
    );
}

/// The writer of the live tree: an exact `--ack-text` strike of the child
/// row `squash and merge` renders its summary tree one level below that row,
/// after the row's own children, with a line break after Guidance. The id
/// hashes the row's lineage (parent-scoped, `#parentscopedanswer`) and the
/// summary.
#[test]
fn an_ack_summary_tree_is_inserted_after_the_rows_children_one_level_below() {
    let before = queue(concat!(
        "- ### https://example.test/pull/375\n",
        "  - %% docs: stop retrying every failure code in the sample %%\n",
        "  - squash and merge\n",
        "    - %% merge with the PR title %%\n",
        "- ### 🔒 https://example.test/pull/376\n",
    ));
    let after = before.replace("  - squash and merge\n", "  - ~~squash and merge~~\n");
    let attached = crate::queue_consume::attach_ack_summary_response(
        &before,
        &after,
        Some(SUMMARY),
        "2026-11-23T03:23:10.000Z",
    )
    .unwrap();
    let id = &agent_doc_hash::content_hash(&format!(
        "ack-summary-row\nhttps://example.test/pull/375: squash and merge\n{SUMMARY}"
    ))[..12];
    let document = attached.document;
    let lines: Vec<&str> = document.lines().collect();
    let at = lines
        .iter()
        .position(|line| *line == "  - ~~squash and merge~~")
        .unwrap();
    assert_eq!(lines[at + 1], "    - %% merge with the PR title %%");
    assert_eq!(lines[at + 2], format!("    - **Response [#r-{id}]**"));
    assert_eq!(
        lines[at + 3],
        "      - **Timestamp:** 2026-11-23T03:23:10.000Z"
    );
    assert_eq!(lines[at + 4], format!("      - **Summary:** {SUMMARY}"));
    assert_eq!(lines[at + 5], "      - **State:** Complete.");
    assert!(
        lines[at + 6].starts_with("      - **Guidance:** "),
        "{document}"
    );
    assert_eq!(lines[at + 7], "- ### 🔒 https://example.test/pull/376");
    assert_eq!(
        slot_response_trees(&document),
        (document.clone(), Vec::new())
    );

    // A struck column-0 row with a comment and a child row takes its tree
    // after both, 2 spaces deep.
    let before = queue(concat!(
        "- ### https://example.test/pull/377\n",
        "  - %% docs: add quickstarts %%\n",
        "  - 📥 squash and merge too\n",
    ));
    let after = before.replace(
        "- ### https://example.test/pull/377\n",
        "- ### ~~https://example.test/pull/377~~\n",
    );
    let document = crate::queue_consume::attach_ack_summary_response(
        &before,
        &after,
        Some("Merged."),
        "2026-11-23T03:23:10.000Z",
    )
    .unwrap()
    .document;
    let lines: Vec<&str> = document.lines().collect();
    let at = lines
        .iter()
        .position(|line| *line == "- ### ~~https://example.test/pull/377~~")
        .unwrap();
    assert_eq!(lines[at + 1], "  - %% docs: add quickstarts %%");
    assert_eq!(lines[at + 2], "  - 📥 squash and merge too");
    assert!(
        lines[at + 3].starts_with("  - **Response [#r-"),
        "{document}"
    );
    assert_eq!(
        slot_response_trees(&document),
        (document.clone(), Vec::new())
    );
}
