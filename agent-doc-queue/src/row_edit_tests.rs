//! Tests for [`super`] (`#patchruncomponent`).

use super::*;

/// The shape of the `#patchruncomponent` repro (agent-doc-bugs.md,
/// 2026-11-25): a struck URL heading with two `%%` notes and a response
/// subtree, in a heading-anchored `agent:run id="main"`, beside a loop.
const DOC: &str = concat!(
    "# Session\n\n",
    "## Run <!-- agent:run id=\"main\" subagents priority go -->\n",
    "- ### ~~🗑️ https://github.com/btakita/agent-doc/issues/435 [#q-7174ad2918b2]~~\n",
    "  - %% 0.35.586: Run Agent Doc and Sync Tmux Layout still fail %%\n",
    "  - %% GH #435 stays open until it is verified on the Coder host after that release. %%\n",
    "  - **Response [#r-7999a0952af9]**\n",
    "    - **State:** Complete.\n",
    "- ### 📥 do [#patchruncomponent]\n",
    "- ### [#q-4172f070eca1] follow-up: operator: check Coder [#q-5ddacb9f0128]\n",
    "<!-- /agent:run -->\n\n",
    "<!-- agent:loop id=\"issues\" -->\n",
    "- %% GH #435 stays open until it is verified on the Coder host after that release. %%\n",
    "<!-- /agent:loop -->\n",
);

const STALE: &str =
    "%% GH #435 stays open until it is verified on the Coder host after that release. %%";
const FRESH: &str =
    "%% closed 2026-11-24; tracked in https://github.com/btakita/agent-doc/issues/442 §2 %%";

fn line_of(document: &str, needle: &str) -> usize {
    document
        .lines()
        .position(|line| line.contains(needle))
        .map(|index| index + 1)
        .unwrap_or_else(|| panic!("{needle:?} not in document"))
}

fn replace(text: &str) -> RowEditOp {
    RowEditOp::Replace(text.to_string())
}

/// The repro: replace the stale note under the `#435` heading, named by the
/// heading's own `[#q-…]` id and the note text, without line numbers.
#[test]
fn find_under_row_replaces_only_that_note() {
    let plan = plan_row_edit(
        DOC,
        Some("main"),
        &RowEditSelector::Find {
            text: STALE.to_string(),
            under: Some("q-7174ad2918b2".to_string()),
        },
        None,
        &replace(FRESH),
    )
    .unwrap();
    assert!(plan.changed);
    assert_eq!(plan.queue, "main");
    assert_eq!(plan.line, line_of(DOC, "  - %% GH #435"));
    assert_eq!(plan.after.as_deref(), Some(format!("  - {FRESH}").as_str()));
    assert_eq!(
        plan.document,
        DOC.replacen(&format!("  - {STALE}"), &format!("  - {FRESH}"), 1),
        "only the one note line changes; the loop's identical note stays"
    );
}

/// Without a row or queue scope the same note text names two lines (one in
/// each queue), so the edit refuses and lists both.
#[test]
fn ambiguous_find_refuses_and_names_the_candidates() {
    let error = plan_row_edit(
        DOC,
        None,
        &RowEditSelector::Find {
            text: format!("- {STALE}"),
            under: None,
        },
        None,
        &replace(FRESH),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("2 lines"), "{error}");
    assert!(
        error.contains(&format!("L{}", line_of(DOC, "  - %% GH #435"))),
        "{error}"
    );

    // `--id issues` narrows it to the loop's line.
    let plan = plan_row_edit(
        DOC,
        Some("issues"),
        &RowEditSelector::Find {
            text: STALE.to_string(),
            under: None,
        },
        None,
        &replace(FRESH),
    )
    .unwrap();
    assert_eq!(plan.queue, "issues");
    assert!(
        plan.document
            .contains(&format!("- {FRESH}\n<!-- /agent:loop -->"))
    );
    assert!(plan.document.contains(&format!("  - {STALE}\n")));
}

#[test]
fn line_selector_with_expect_guards_against_a_shifted_line() {
    let line = line_of(DOC, "  - %% GH #435");
    let plan = plan_row_edit(
        DOC,
        None,
        &RowEditSelector::Line(line),
        Some(STALE),
        &replace(FRESH),
    )
    .unwrap();
    assert!(plan.changed);

    // The operator inserted a row above: line N now holds a different line.
    let shifted = DOC.replacen("- ### 📥", "- new operator row\n- ### 📥", 1);
    let shifted = shifted.replacen(
        "## Run <!-- agent:run id=\"main\" subagents priority go -->\n",
        "## Run <!-- agent:run id=\"main\" subagents priority go -->\n- typed above\n",
        1,
    );
    let error = plan_row_edit(
        &shifted,
        None,
        &RowEditSelector::Line(line),
        Some(STALE),
        &replace(FRESH),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("no longer reads"), "{error}");
}

#[test]
fn row_selector_matches_the_rows_own_id_not_a_follow_up_reference() {
    // `q-4172f070eca1` appears only as a follow-up's origin reference, which
    // is never that row's own id.
    let error = plan_row_edit(
        DOC,
        None,
        &RowEditSelector::Row("q-4172f070eca1".to_string()),
        None,
        &replace("x"),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("no row"), "{error}");

    // The follow-up's own trailing id selects it; `[#…]` and `#…` spellings too.
    for spelling in ["q-5ddacb9f0128", "#q-5ddacb9f0128", "[#q-5ddacb9f0128]"] {
        let plan = plan_row_edit(
            DOC,
            None,
            &RowEditSelector::Row(spelling.to_string()),
            None,
            &replace(
                "### [#q-4172f070eca1] follow-up: operator: check Coder again [#q-5ddacb9f0128]",
            ),
        )
        .unwrap();
        assert_eq!(plan.line, line_of(DOC, "follow-up: operator: check Coder"));
        assert!(
            plan.document.contains(
                "- ### [#q-4172f070eca1] follow-up: operator: check Coder again [#q-5ddacb9f0128]\n"
            ),
            "{}",
            plan.document
        );
    }

    // A struck heading's generated id and an authored `do [#id]` both select.
    let struck = plan_row_edit(
        DOC,
        None,
        &RowEditSelector::Row("q-7174ad2918b2".to_string()),
        None,
        &replace("### ~~🗑️ https://github.com/btakita/agent-doc/issues/435 [#q-7174ad2918b2]~~"),
    )
    .unwrap();
    assert!(!struck.changed, "same text is a no-op");
    let authored = plan_row_edit(
        DOC,
        None,
        &RowEditSelector::Row("patchruncomponent".to_string()),
        None,
        &replace("### 📥 do [#patchruncomponent] (now with notes)"),
    )
    .unwrap();
    assert_eq!(authored.line, line_of(DOC, "do [#patchruncomponent]"));
}

#[test]
fn replacement_keeps_indent_and_marker_unless_it_brings_its_own() {
    let selector = RowEditSelector::Find {
        text: STALE.to_string(),
        under: Some("q-7174ad2918b2".to_string()),
    };
    let own_marker = plan_row_edit(
        DOC,
        Some("main"),
        &selector,
        None,
        &replace(&format!("* {FRESH}")),
    )
    .unwrap();
    assert_eq!(
        own_marker.after.as_deref(),
        Some(format!("  * {FRESH}").as_str())
    );
}

#[test]
fn delete_removes_one_leaf_line_and_refuses_a_line_with_children() {
    let plan = plan_row_edit(
        DOC,
        Some("main"),
        &RowEditSelector::Find {
            text: STALE.to_string(),
            under: Some("q-7174ad2918b2".to_string()),
        },
        None,
        &RowEditOp::Delete,
    )
    .unwrap();
    assert_eq!(plan.after, None);
    assert_eq!(
        plan.document,
        DOC.replacen(&format!("  - {STALE}\n"), "", 1)
    );

    let error = plan_row_edit(
        DOC,
        None,
        &RowEditSelector::Row("q-7174ad2918b2".to_string()),
        None,
        &RowEditOp::Delete,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("nested line(s)"), "{error}");
    assert!(error.contains("delete-tree"), "{error}");
}

#[test]
fn markers_multiline_blank_and_out_of_body_lines_refuse() {
    let note = RowEditSelector::Find {
        text: STALE.to_string(),
        under: Some("q-7174ad2918b2".to_string()),
    };
    for (text, needle) in [
        ("<!-- /agent:run -->", "component"),
        ("<!-- agent:todo -->", "component"),
        ("one\ntwo", "one line"),
        ("   ", "blank"),
    ] {
        let error = plan_row_edit(DOC, None, &note, None, &replace(text))
            .unwrap_err()
            .to_string();
        assert!(error.contains(needle), "{text:?}: {error}");
    }
    // The opening marker line and text outside any queue are not editable.
    for line in [line_of(DOC, "## Run <!--"), 1] {
        let error = plan_row_edit(DOC, None, &RowEditSelector::Line(line), None, &replace("x"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("not inside the body"), "{error}");
    }
    let error = plan_row_edit(DOC, Some("missing"), &note, None, &replace("x"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("queue id `missing` not found"), "{error}");
}

#[test]
fn crlf_line_endings_are_preserved() {
    let doc = DOC.replace('\n', "\r\n");
    let plan = plan_row_edit(
        &doc,
        Some("main"),
        &RowEditSelector::Find {
            text: STALE.to_string(),
            under: Some("q-7174ad2918b2".to_string()),
        },
        None,
        &replace(FRESH),
    )
    .unwrap();
    assert_eq!(
        plan.document,
        doc.replacen(&format!("  - {STALE}\r\n"), &format!("  - {FRESH}\r\n"), 1)
    );
}
