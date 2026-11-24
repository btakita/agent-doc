//! `#pipeprompt`: a queue row ending in ` |` continues into its children.
//!
//! The chain of ` |` rows plus their terminal leaves is ONE structured prompt
//! with one queue head, the top ` |` row. Without ` |`, children stay separate
//! leaves.

use super::*;

const STATEMENT: &str = "\
- Collect the open agent-doc GitHub issues |
  - group them by component |
    - draft a triage summary |
      - under 200 words
      - no internal ids
- next item
";

const WITHOUT_PIPES: &str = "\
- Collect the open agent-doc GitHub issues
  - group them by component
    - draft a triage summary
      - under 200 words
      - no internal ids
- next item
";

fn parsed(body: &str) -> Vec<QueueEntry> {
    parse(body).unwrap()
}

fn queue_doc(body: &str) -> String {
    format!("<!-- agent:run -->\n{body}<!-- /agent:run -->\n")
}

#[test]
fn a_pipe_statement_folds_into_one_head() {
    let entries = parsed(STATEMENT);
    assert_eq!(entries.len(), 2, "{entries:?}");
    assert_eq!(executable_prompt_indexes(&entries), vec![0, 1]);
    assert!(entry_is_pipe_statement(&entries, 0));
    assert!(!entry_is_group_parent(&entries, 0));
    let QueueEntry::Prompt(head) = &entries[0] else {
        panic!("{entries:?}");
    };
    assert!(
        head.text.ends_with("      - no internal ids"),
        "{}",
        head.text
    );
    // The markdown-ast enumerator folds the same span.
    let nodes =
        agent_doc_markdown_ast::mutations::item_nodes(&queue_doc(STATEMENT), "queue").unwrap();
    assert_eq!(nodes.len(), 2);
}

#[test]
fn without_the_pipe_children_stay_separate_leaves() {
    let entries = parsed(WITHOUT_PIPES);
    assert_eq!(entries.len(), 6);
    assert_eq!(executable_prompt_indexes(&entries), vec![3, 4, 5]);
    assert!(!entry_is_pipe_statement(&entries, 0));
}

#[test]
fn render_is_byte_for_byte() {
    for body in [
        STATEMENT,
        WITHOUT_PIPES,
        "- #review |\n  - #address\n",
        "- epic\n  - Collect |\n\n    - leaf\n  - sibling\n",
        "- Collect |\n  - group\n    - draft\n",
    ] {
        // Top-level rows gain their `### ` navigation heading once; every
        // other byte, the statement's children included, is kept.
        let canonical = render(&parsed(body));
        assert_eq!(canonical.replace("- ### ", "- "), body);
        assert_eq!(render(&parsed(&canonical)), canonical);
        assert_eq!(parsed(&canonical).len(), parsed(body).len());
    }
}

#[test]
fn strict_grammar_an_inner_parent_without_a_pipe_is_not_a_statement() {
    // `group` has children but does not end in ` |`: membership is explicit
    // at every level, so the statement is malformed and nothing folds.
    let body = "- Collect |\n  - group\n    - draft\n";
    let entries = parsed(body);
    assert_eq!(entries.len(), 3);
    assert_eq!(executable_prompt_indexes(&entries), vec![2]);
    let findings = agent_doc_markdown_ast::overlay::pipe_statement_lints(&queue_doc(body));
    assert!(
        findings.iter().any(|finding| finding.kind
            == agent_doc_markdown_ast::overlay::PipeLintKind::UnpipedStageParent
            && finding.path == "Collect | group"),
        "{findings:?}"
    );
}

#[test]
fn a_statement_with_only_its_top_row_piped_folds_its_leaves() {
    let body = "- draft a triage summary |\n  - under 200 words\n  - no internal ids\n";
    let entries = parsed(body);
    assert_eq!(entries.len(), 1);
    assert_eq!(executable_prompt_indexes(&entries), vec![0]);
}

#[test]
fn toggling_the_pipe_keeps_row_identity() {
    for (with, without, index) in [
        ("- Collect the issues |\n", "- Collect the issues\n", 0),
        (
            "- Collect the issues | [#q-0123456789ab]\n",
            "- Collect the issues [#q-0123456789ab]\n",
            0,
        ),
        ("- epic\n  - Collect |\n", "- epic\n  - Collect\n", 1),
        (
            "- Collect the issues | [#q-0123456789ab]\n  - leaf\n",
            "- Collect the issues [#q-0123456789ab]\n  - leaf\n",
            0,
        ),
    ] {
        let piped = parsed(with);
        let plain = parsed(without);
        assert_eq!(
            crate::queue_claim::claim_identity_at(&piped, index),
            crate::queue_claim::claim_identity_at(&plain, index),
            "{with:?}"
        );
    }
    assert_eq!(
        agent_doc_element_queue::queue_head_identity("Collect | [#q-0123456789ab]"),
        agent_doc_element_queue::queue_head_identity("Collect [#q-0123456789ab]")
    );
    assert_eq!(
        agent_doc_element_queue::queue_head_identity("Collect |"),
        agent_doc_element_queue::queue_head_identity("Collect")
    );
}

#[test]
fn one_q_id_on_the_top_row_and_none_inside() {
    let doc = queue_doc(STATEMENT);
    let projected = crate::prompt_identity::project(&doc, "doc");
    let head = projected
        .content
        .lines()
        .find(|line| line.starts_with("- Collect"))
        .unwrap();
    assert!(
        head.starts_with("- Collect the open agent-doc GitHub issues | [#q-"),
        "{head}"
    );
    assert_eq!(
        projected
            .content
            .lines()
            .filter(|line| line.contains("[#q-"))
            .count(),
        2,
        "the statement head and `next item` only:\n{}",
        projected.content
    );
    // The admitted q-id leaves the statement intact.
    let body = projected
        .content
        .strip_prefix("<!-- agent:run -->\n")
        .unwrap()
        .strip_suffix("<!-- /agent:run -->\n")
        .unwrap();
    let entries = parsed(body);
    assert_eq!(entries.len(), 2);
    assert!(entry_is_pipe_statement(&entries, 0));
}

#[test]
fn inner_rows_are_never_offered_or_claimed() {
    let entries = parsed(STATEMENT);
    for inner in [
        "group them by component |",
        "draft a triage summary |",
        "under 200 words",
    ] {
        assert!(
            executable_prompt_indexes(&entries)
                .iter()
                .all(|&index| !row_matches_head_text(&entries, index, inner)),
            "{inner}"
        );
    }
    // A claimed child refuses the fold: adding ` |` above it never takes
    // that claim's row away.
    let claimed = "- Collect |\n  - 🚧 [#subagent:x#1] group them\n";
    let entries = parsed(claimed);
    assert_eq!(entries.len(), 2);
    assert_eq!(executable_prompt_indexes(&entries), vec![1]);
    let findings = agent_doc_markdown_ast::overlay::pipe_statement_lints(&queue_doc(claimed));
    assert!(
        findings.iter().any(|finding| finding.kind
            == agent_doc_markdown_ast::overlay::PipeLintKind::ClaimedRowInStatement),
        "{findings:?}"
    );
}

#[test]
fn strike_and_response_attach_to_the_top_row() {
    let doc = queue_doc(STATEMENT);
    let nodes = agent_doc_markdown_ast::mutations::item_nodes(&doc, "queue").unwrap();
    let closed =
        agent_doc_markdown_ast::mutations::consume_node(&doc, "queue", &nodes[0].node_key).unwrap();
    assert!(
        closed.contains(concat!(
            "- ### ~~Collect the open agent-doc GitHub issues |~~\n",
            "  - ~~group them by component |~~\n",
            "    - ~~draft a triage summary |~~\n",
            "      - ~~under 200 words~~\n",
            "      - ~~no internal ids~~\n",
            "- next item"
        )),
        "{closed}"
    );
    let body = closed
        .strip_prefix("<!-- agent:run -->\n")
        .unwrap()
        .strip_suffix("<!-- /agent:run -->\n")
        .unwrap();
    let entries = parsed(body);
    assert_eq!(entries.len(), 2, "{entries:?}");
    assert!(matches!(entries[0], QueueEntry::Completed(_)));
    assert_eq!(executable_prompt_indexes(&entries), vec![1]);

    // A response tree under the head sits after the statement and stays
    // generated metadata, never statement text.
    let answered = "- 🟡 Collect |\n  - leaf\n  - **Response [#r-0123456789ab]**\n    - **Summary:** done\n- next\n";
    let entries = parsed(answered);
    let QueueEntry::Prompt(head) = &entries[0] else {
        panic!("{entries:?}");
    };
    assert_eq!(head.text, "🟡 Collect |\n  - leaf");
    assert!(entry_is_pipe_statement(&entries, 0));
    assert_eq!(render(&entries).replace("- ### ", "- "), answered);
}

#[test]
fn commands_pipe_too() {
    let body = "- #review |\n  - #address\n";
    let entries = parsed(body);
    assert_eq!(entries.len(), 1);
    assert_eq!(executable_prompt_indexes(&entries), vec![0]);
    let submitted = submitted_prompt_at(&entries, 0).unwrap();
    assert!(
        submitted.starts_with("#review |\n  - #address\n"),
        "{submitted}"
    );
    assert!(submitted.ends_with("\n#review | #address"), "{submitted}");
}

#[test]
fn submission_is_one_task_with_its_tree_intact() {
    let entries = parsed(STATEMENT);
    let submitted = submitted_prompt_at(&entries, 0).unwrap();
    assert_eq!(
        submitted,
        format!(
            "{}\n{PIPE_PROMPT_HEADER}\n{}",
            STATEMENT
                .strip_prefix("- ")
                .and_then(|body| body.strip_suffix("\n- next item\n"))
                .unwrap(),
            "Collect the open agent-doc GitHub issues | group them by component | draft a triage summary | under 200 words | no internal ids",
        )
    );
    assert!(!submitted.contains(PROMPT_TREE_HEADER));
}

#[test]
fn inner_paths_join_with_pipe_and_outer_paths_with_colon() {
    let body = "- release epic\n  - Collect the issues |\n    - group them |\n      - draft\n  - sibling\n";
    let entries = parsed(body);
    assert_eq!(entries.len(), 3, "{entries:?}");
    assert_eq!(executable_prompt_indexes(&entries), vec![1, 2]);
    let submitted = submitted_prompt_at(&entries, 1).unwrap();
    assert!(
        submitted
            .starts_with("release epic: Collect the issues |\n    - group them |\n      - draft\n"),
        "{submitted}"
    );
    assert!(
        submitted.ends_with("\nrelease epic: Collect the issues | group them | draft"),
        "{submitted}"
    );
}

#[test]
fn lints_report_misplaced_pipes() {
    use agent_doc_markdown_ast::overlay::{PipeLintKind, pipe_statement_lints};
    let doc = queue_doc("- lonely |\n- run `ls |`\n- Collect ||\n- pending | completed\n");
    let kinds = pipe_statement_lints(&doc)
        .into_iter()
        .map(|finding| finding.kind)
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        vec![
            PipeLintKind::PipeWithoutChildren,
            PipeLintKind::PipeInCodeSpan,
            PipeLintKind::ReservedEdgeOperator,
        ]
    );
    assert!(pipe_statement_lints(&queue_doc(STATEMENT)).is_empty());
    // Outside a runnable component nothing is linted.
    assert!(pipe_statement_lints("- lonely |\n").is_empty());
}

const FOLD: &str = "\
- Summarize the release thread >
  - focus on blockers
    - especially CI
  - skip chit-chat
- next item
";

#[test]
fn a_fold_is_one_task_with_its_whole_list_as_context() {
    let entries = parsed(FOLD);
    assert_eq!(entries.len(), 2, "{entries:?}");
    assert_eq!(executable_prompt_indexes(&entries), vec![0, 1]);
    assert!(entry_is_edge_block(&entries, 0));
    assert!(!entry_is_pipe_statement(&entries, 0));
    let submitted = submitted_prompt_at(&entries, 0).unwrap();
    assert_eq!(
        submitted,
        format!(
            "Summarize the release thread >\n  - focus on blockers\n    - especially CI\n  - skip chit-chat\n{FOLD_PROMPT_HEADER}"
        )
    );
    // Without ` >` the same rows are context for separate leaves.
    let plain = parsed(&FOLD.replace(" >\n", "\n"));
    assert_eq!(executable_prompt_indexes(&plain), vec![2, 3, 4]);
    // Markdown-ast folds the same span.
    let nodes = agent_doc_markdown_ast::mutations::item_nodes(&queue_doc(FOLD), "queue").unwrap();
    assert_eq!(nodes.len(), 2);
}

#[test]
fn a_fold_gets_one_q_id_keeps_identity_and_renders_byte_for_byte() {
    let projected = crate::prompt_identity::project(&queue_doc(FOLD), "doc");
    assert_eq!(
        projected
            .content
            .lines()
            .filter(|line| line.contains("[#q-"))
            .count(),
        2,
        "{}",
        projected.content
    );
    assert!(
        projected
            .content
            .contains("- Summarize the release thread > [#q-"),
        "{}",
        projected.content
    );
    let canonical = render(&parsed(FOLD));
    assert_eq!(canonical.replace("- ### ", "- "), FOLD);
    assert_eq!(render(&parsed(&canonical)), canonical);
    for (with, without) in [
        ("- Summarize >\n", "- Summarize\n"),
        (
            "- Summarize > [#q-0123456789ab]\n  - context\n",
            "- Summarize [#q-0123456789ab]\n  - context\n",
        ),
    ] {
        assert_eq!(
            crate::queue_claim::claim_identity_at(&parsed(with), 0),
            crate::queue_claim::claim_identity_at(&parsed(without), 0),
            "{with:?}"
        );
    }
    // A claimed child refuses the fold.
    let claimed = parsed("- Summarize >\n  - 🚧 [#subagent:x#1] context\n");
    assert_eq!(claimed.len(), 2);
    assert_eq!(executable_prompt_indexes(&claimed), vec![1]);
}

#[test]
fn fold_and_reserved_operator_lints() {
    use agent_doc_markdown_ast::overlay::{PipeLintKind, pipe_statement_lints};
    let findings = pipe_statement_lints(&queue_doc(
        "- lonely >\n- Collect |*\n- Collect |>\n- Collect |+\n- a -> b\n",
    ));
    let kinds = findings
        .iter()
        .map(|finding| finding.kind)
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        vec![
            PipeLintKind::FoldWithoutChildren,
            PipeLintKind::ReservedEdgeOperator,
            PipeLintKind::ReservedEdgeOperator,
            PipeLintKind::ReservedEdgeOperator,
        ]
    );
    assert!(
        findings[1]
            .message
            .contains("did you mean ` |+` (deep pipe)?")
    );
    assert!(findings[2].message.contains("hand-off pipe"));
    assert!(pipe_statement_lints(&queue_doc(FOLD)).is_empty());
    // A ` >` stage inside a statement folds its own list.
    assert!(
        pipe_statement_lints(&queue_doc("- Collect |\n  - summarize >\n    - context\n"))
            .is_empty()
    );
}

fn edge_claim(item_text: &str, state: &str) -> crate::queue_claim::QueueClaim {
    serde_json::from_value(serde_json::json!({
        "identity": crate::queue_claim::claim_identity(item_text),
        "item_text": item_text,
        "owner": "subagent:pipe",
        "generation": 3,
        "state": state,
        "claimed_at_secs": 1,
        "expires_at_secs": 2,
    }))
    .unwrap()
}

#[test]
fn removing_the_operator_from_a_claimed_head_is_refused() {
    use crate::queue_mirror_projection::restore_claimed_edge_markers;
    let claim = edge_claim(
        "Collect the issues | [#q-0123456789ab]\n  - leaf",
        "running",
    );
    let edited = queue_doc(
        "- ### 🚧 [#subagent:pipe#3] Collect the issues [#q-0123456789ab]\n  - leaf\n- next\n",
    );
    let restored = restore_claimed_edge_markers(&edited, std::slice::from_ref(&claim))
        .unwrap()
        .expect("the claimed head keeps its ` |`");
    assert_eq!(
        restored,
        queue_doc(
            "- ### 🚧 [#subagent:pipe#3] Collect the issues | [#q-0123456789ab]\n  - leaf\n- next\n"
        )
    );
    // Swapping ` |` for ` >` is refused the same way.
    let swapped = queue_doc("- 🚧 [#subagent:pipe#3] Collect the issues >\n  - leaf\n");
    assert_eq!(
        restore_claimed_edge_markers(&swapped, std::slice::from_ref(&claim)).unwrap(),
        Some(queue_doc(
            "- 🚧 [#subagent:pipe#3] Collect the issues |\n  - leaf\n"
        ))
    );
    // Untouched: the operator is still there, the attempt is lost, another
    // attempt owns the row, or the claim was taken without an operator.
    let intact = queue_doc("- 🚧 [#subagent:pipe#3] Collect the issues |\n  - leaf\n");
    assert_eq!(
        restore_claimed_edge_markers(&intact, std::slice::from_ref(&claim)).unwrap(),
        None
    );
    let lost = edge_claim("Collect the issues |\n  - leaf", "lost");
    assert_eq!(
        restore_claimed_edge_markers(&edited, &[lost]).unwrap(),
        None
    );
    let other = queue_doc("- 🚧 [#subagent:pipe#4] Collect the issues\n  - leaf\n");
    assert_eq!(
        restore_claimed_edge_markers(&other, std::slice::from_ref(&claim)).unwrap(),
        None
    );
    let plain = edge_claim("Collect the issues", "running");
    assert_eq!(
        restore_claimed_edge_markers(&edited, &[plain]).unwrap(),
        None
    );
}
