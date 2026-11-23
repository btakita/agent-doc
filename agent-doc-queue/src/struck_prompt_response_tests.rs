use super::*;

#[test]
fn questions_and_short_answers_are_reply_shaped() {
    assert!(is_operator_reply_shape("Is this done?"));
    assert!(is_operator_reply_shape("👀 Is this done?"));
    assert!(is_operator_reply_shape("Apache-2.0"));
    assert!(is_operator_reply_shape(
        "~~Apache-2.0 — auto-struck: completed by #decideapi (#qftbklgstrike)~~"
    ));
    assert!(is_operator_reply_shape(
        "~~🗑\u{fe0f} Since we set up the token, can you use the workflow to add this now?~~"
    ));
    assert!(is_operator_reply_shape("yes [#q-0123456789ab]"));
    assert!(!is_operator_reply_shape(
        "Fix JB File Cache Conflict dialogs on every save"
    ));
    assert!(!is_operator_reply_shape("Repair cache duplication on save"));
}

#[test]
fn row_prose_strips_binary_decorations_only() {
    assert_eq!(
        row_prose("~~🗑\u{fe0f} Is this done? — auto-struck: completed by #x (#qftbklgstrike)~~"),
        "Is this done?"
    );
    assert_eq!(
        row_prose("Choose the license [operator-verify] [#q-6bbeb70951ea]"),
        "Choose the license"
    );
}

#[test]
fn abbreviated_response_is_one_tree_under_the_row() {
    let lines = abbreviated_strike_response_lines("Fix the parser", 2, "Already done:\n #fix.");
    assert_eq!(lines.len(), 3);
    assert!(lines[0].starts_with("    - **Response [#r-"), "{lines:?}");
    assert!(
        agent_doc_markdown_ast::exchange_tree::is_compact_response_id(
            lines[0]
                .trim()
                .trim_start_matches("- **Response ")
                .trim_end_matches("**")
        )
    );
    assert_eq!(lines[1], "      - **Summary:** Already done: #fix.");
    assert_eq!(lines[2], "      - **State:** Complete.");
    assert_eq!(
        strike_response_id("Fix the parser"),
        strike_response_id(" Fix the parser ")
    );
}

/// The three live 2026-11-22 shapes (private names replaced): two operator
/// replies the `#qftbklgstrike` semantic strike buried as "completed by" an
/// unrelated id, and an operator question struck with no response at all.
const LIVE_SHAPES: &str = concat!(
    "## scratch <!-- agent:run id=\"scratch\" subagents keep go -->\n",
    "- ### 🙋 🔒 follow up: operator: decide whether code comments that cite private ids count as an IP issue. [operator-verify] [#q-5296275f2730]\n",
    "  - ~~Is this done? — auto-struck: completed by #installplugin (#qftbklgstrike)~~\n",
    "- ### Choose the C++ SDK license before the first cpp zip ships.\n",
    "  - ~~Apache-2.0 — auto-struck: completed by #decideapi (#qftbklgstrike)~~\n",
    "- ### 🙋 🔒 follow up: human (repository maintainer): set the token expiry variable. [operator-verify] [#q-ba8b54170b82]\n",
    "  - 🙋 Use the browser bridge to run the workflow with the current token. [operator-verify]\n",
    "    - ~~🗑\u{fe0f} Since we set up the token, can you use the workflow to add this now?~~\n",
    "- ### ~~How do I mint the token?~~\n",
    "  - **Response [#r-7ec8bf1541c3]**\n",
    "    - **Summary:** At the token settings page with only the repo scope.\n",
    "    - **State:** Complete.\n",
    "- ~~Fix JB File Cache Conflict dialogs on every save — auto-struck: completed by #jbcache (#qftbklgstrike)~~\n",
    "<!-- /agent:run -->\n",
);

#[test]
fn reports_every_unanswered_struck_operator_prompt_and_nothing_else() {
    let rows = unanswered_struck_prompt_rows(LIVE_SHAPES);
    let summary = rows
        .iter()
        .map(|row| (row.line, row.reason))
        .collect::<Vec<_>>();
    assert_eq!(
        summary,
        vec![
            (3, UnansweredStruckReason::SemanticStrikeOfReply),
            (5, UnansweredStruckReason::SemanticStrikeOfReply),
            (8, UnansweredStruckReason::QuestionWithoutResponse),
        ],
        "{rows:#?}"
    );
    assert_eq!(
        rows[0].row,
        "- ~~Is this done? — auto-struck: completed by #installplugin (#qftbklgstrike)~~"
    );
    let rendered = rows[2].to_string();
    assert!(rendered.contains("#strucknoresponse"), "{rendered}");
    assert!(rendered.contains("line 8"), "{rendered}");
    assert!(rendered.contains("un-strikes"), "{rendered}");
}

#[test]
fn a_struck_question_with_its_response_is_answered() {
    let document = concat!(
        "<!-- agent:queue -->\n",
        "- ~~Is this done?~~\n",
        "  - **Response [#r-0123456789ab]**\n",
        "    - **Summary:** Yes.\n",
        "<!-- /agent:queue -->\n",
    );
    assert!(unanswered_struck_prompt_rows(document).is_empty());

    let consumed = concat!(
        "<!-- agent:queue -->\n",
        "- ~~Is this done?~~\n",
        "  - **Consumed by:** [#r-0123456789ab]. You can delete this subtree.\n",
        "<!-- /agent:queue -->\n",
    );
    assert!(unanswered_struck_prompt_rows(consumed).is_empty());
}
