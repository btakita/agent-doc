//! Tests for [`super`] (`#treestatusquery`).

use super::*;

fn queue_doc(body: &str) -> String {
    format!("<!-- agent:queue id=\"main\" -->\n{body}<!-- /agent:queue -->\n")
}

/// 1-based line of the first document line containing `needle`.
fn line_of(document: &str, needle: &str) -> usize {
    document
        .lines()
        .position(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("{needle} not in:\n{document}"))
        + 1
}

const MIXED: &str = concat!(
    "- 🚧 [#subagent:alpha#1] do [#alpha]\n",
    "- do [#beta]\n",
    "- ~~do [#gamma]~~ — auto-struck: response: [#r-0123456789ab]\n",
    "  - **Response [#r-0123456789ab]**\n",
    "    - **Timestamp:** 2026-11-01T12:00:00Z\n",
    "    - **Summary:** Done.\n",
    "    - **State:** Complete.\n",
    "    - **Guidance:** Safe to delete this completed tree; the item is complete and has no remaining tracked or unpublished work.\n",
    "  - ~~do [#gamma-child]~~\n",
    "- ~~do [#delta]~~ — auto-struck: response: [#r-abcdef012345]\n",
    "  - **Response [#r-abcdef012345]**\n",
    "    - **Timestamp:** 2026-11-01T12:00:00Z\n",
    "    - **Summary:** Needs a decision.\n",
    "    - **State:** Waiting on operator input.\n",
    "    - **Guidance:** More work remains. Keep this tree and answer the operator request below.\n",
    "- ⏭️ do [#epsilon]\n",
    "- ~~do [#zeta]~~\n",
    "  - do [#zeta-child]\n",
);

#[test]
fn whole_queue_lists_you_agent_deletable_and_review_rows() {
    let document = queue_doc(MIXED);
    let status = tree_status(&document, 1).unwrap();
    assert!(status.whole_queue, "{status:#?}");
    assert_eq!(status.queue, "main");
    assert_eq!(status.you.len(), 1, "{status:#?}");
    assert_eq!(status.you[0].id.as_deref(), Some("delta"));
    let agent_ids = status
        .agent
        .iter()
        .map(|row| row.id.as_deref().unwrap_or_default())
        .collect::<Vec<_>>();
    assert_eq!(
        agent_ids,
        vec!["alpha", "beta", "zeta-child"],
        "{status:#?}"
    );
    assert_eq!(
        status.agent[0].action,
        "let the running attempt finish and report"
    );
    assert_eq!(status.agent[1].action, "pick up this queued item");
    // Only the outermost deletable subtree is listed; a struck row with a live
    // child, or whose response waits on the operator, is not deletable.
    assert_eq!(status.deletable.len(), 1, "{status:#?}");
    let gamma = &status.deletable[0];
    assert_eq!(gamma.id.as_deref(), Some("gamma"));
    assert_eq!(gamma.line, line_of(&document, "do [#gamma]~~"));
    assert_eq!(gamma.end_line, line_of(&document, "do [#gamma-child]"));
    assert_eq!(status.needs_review.len(), 1, "{status:#?}");
    assert_eq!(status.needs_review[0].id.as_deref(), Some("epsilon"));
    assert_eq!(
        status.counts_line(),
        "1 you · 3 agent · 1 deletable · 1 need review"
    );
}

#[test]
fn a_line_selects_the_deepest_row_whose_subtree_holds_it() {
    let document = queue_doc(MIXED);
    // A line inside the struck row's response summary selects that row.
    let status = tree_status(&document, line_of(&document, "Summary:** Done.")).unwrap();
    assert!(!status.whole_queue);
    assert_eq!(status.id.as_deref(), Some("gamma"));
    assert!(
        status.you.is_empty() && status.agent.is_empty(),
        "{status:#?}"
    );
    assert_eq!(status.deletable.len(), 1);

    let status = tree_status(&document, line_of(&document, "do [#zeta]~~")).unwrap();
    assert_eq!(status.id.as_deref(), Some("zeta"));
    assert_eq!(status.agent.len(), 1, "{status:#?}");
    assert!(status.deletable.is_empty(), "a live child keeps the tree");

    assert!(tree_status("# no queue\n", 1).is_err());
}

#[test]
fn unpublished_and_follow_up_responses_are_not_deletable() {
    let document = queue_doc(concat!(
        "- ~~do [#local]~~ — auto-struck: response: [#r-0123456789ab]\n",
        "  - **Response [#r-0123456789ab]**\n",
        "    - **State:** Unpublished local work remains.\n",
        "- ~~do [#follow]~~ — auto-struck: response: [#r-abcdef012345]\n",
        "  - **Response [#r-abcdef012345]**\n",
        "    - **State:** Follow-up work remains.\n",
    ));
    let status = tree_status(&document, 1).unwrap();
    assert!(status.deletable.is_empty(), "{status:#?}");
    assert_eq!(status.agent.len(), 1);
    assert_eq!(status.agent[0].id.as_deref(), Some("local"));
    assert_eq!(status.needs_review.len(), 1);
    assert_eq!(status.needs_review[0].id.as_deref(), Some("follow"));
}

const WAITING: &str = concat!(
    "- ### Waiting on you — answer by adding a child line under an item <!-- agent-doc:waiting-on-you -->\n",
    "  - [#ask] Choose an owner. <!-- agent-doc:waiting-item -->\n",
    "    - **Response:** Choose an owner. ▫️\n",
    "      - **Question:** Who owns the alpha release? ▫️\n",
    "  - [#answered] Pick a port. <!-- agent-doc:waiting-item -->\n",
    "    - use 8080\n",
);

#[test]
fn waiting_items_use_their_derived_next_action() {
    let document = format!(
        "{}<!-- agent:todo -->\n- [ ] [#ask] Choose an owner.\n- [ ] [#answered] Pick a port.\n<!-- /agent:todo -->\n",
        queue_doc(WAITING)
    );
    let status = tree_status(&document, 1).unwrap();
    assert_eq!(status.you.len(), 1, "{status:#?}");
    assert_eq!(status.you[0].id.as_deref(), Some("ask"));
    assert_eq!(
        status.you[0].action,
        "answer: Who owns the alpha release? (evidence: Question)"
    );
    assert_eq!(status.agent.len(), 1, "{status:#?}");
    assert_eq!(status.agent[0].id.as_deref(), Some("answered"));
    assert!(
        status.agent[0]
            .action
            .starts_with("pick up your reply “use 8080”")
    );
    // The operator's reply row is part of its item, never listed twice.
    assert!(status.needs_review.is_empty(), "{status:#?}");
}

#[test]
fn a_packet_row_is_triaged_as_its_own_subtree() {
    let body = concat!(
        "- [#rel] Choose a release. <!-- agent-doc:waiting-item -->\n",
        "  - **Request**: [#subagent-operator-request-rel-pick]\n",
        "  - option one\n",
        "    - ☑️ [#subagent:one#1] Resolve the open operator action to complete the parent item. [#rel]\n",
        "  - option two\n",
        "    - ~~tried and dropped~~\n",
    );
    let document = format!(
        "{}<!-- agent:todo -->\n- [ ] [#rel] Choose a release.\n- [ ] [#subagent-operator-request-rel-pick] Pick.\n<!-- /agent:todo -->\n",
        queue_doc(body)
    );
    let status = tree_status(&document, line_of(&document, "option one")).unwrap();
    assert_eq!(status.agent.len(), 1, "{status:#?}");
    assert!(
        status.agent[0]
            .action
            .contains("integrate 1 reported result")
    );
    let status = tree_status(&document, line_of(&document, "option two")).unwrap();
    assert_eq!(status.deletable.len(), 1, "{status:#?}");
    assert_eq!(status.deletable[0].line, line_of(&document, "option two"));
}

#[test]
fn render_text_names_every_section() {
    let document = queue_doc(MIXED);
    let text = render_text(&tree_status(&document, 1).unwrap());
    assert!(text.starts_with("L1–L"), "{text}");
    assert!(
        text.contains("queue main — 1 you · 3 agent · 1 deletable · 1 need review"),
        "{text}"
    );
    assert!(text.contains("Next: you:\n"), "{text}");
    assert!(text.contains("Deletable:\n"), "{text}");
    assert!(text.contains("status? deep"), "{text}");
}

fn prompts(document: &str) -> Vec<String> {
    let component = element::parse(document)
        .unwrap()
        .into_iter()
        .find(|component| component.name == "queue")
        .unwrap();
    let entries = document_queue::parse(component.content(document)).unwrap();
    document_queue::prompts(&entries)
        .into_iter()
        .map(|prompt| prompt.text.clone())
        .collect()
}

#[test]
fn a_status_child_line_is_inert_queue_text() {
    let document = queue_doc("- do [#alpha]\n  - status?\n  - Status? deep\n");
    assert_eq!(prompts(&document), vec!["do [#alpha]".to_string()]);
    assert!(!crate::free_text_admission::free_text_prompt_is_backlog_task("status?"));
}

#[test]
fn a_child_query_becomes_status_rows_and_never_a_prompt() {
    let body = "- ~~do [#zeta]~~\n  - do [#zeta-child]\n  - status?\n- do [#other]\n";
    let document = queue_doc(body);
    let answered = answer_status_queries(&document).unwrap().unwrap();
    assert_eq!(answered.answered.len(), 1);
    assert_eq!(
        answered.answered[0].counts,
        "0 you · 1 agent · 0 deletable · 0 need review"
    );
    let expected = queue_doc(concat!(
        "- ~~do [#zeta]~~\n",
        "  - do [#zeta-child]\n",
        "  - **Status:** snapshot — 0 you · 1 agent · 0 deletable · 0 need review ▫️\n",
        "    - **Next: agent:** “do `#zeta-child`” — pick up this queued item ▫️\n",
        "- do [#other]\n",
    ));
    assert_eq!(answered.content, expected);
    // The Status rows are inert: no new prompt, and a second pass is a no-op.
    assert_eq!(
        prompts(&answered.content),
        vec!["do [#zeta-child]".to_string(), "do [#other]".to_string()]
    );
    assert!(answer_status_queries(&answered.content).unwrap().is_none());
    // The queue tree does not list them as rows either.
    let tree = queue_tree::queue_tree(&answered.content, &QueueTreeOptions::default()).unwrap();
    assert_eq!(tree.nodes.len(), 3, "{tree:#?}");
}

#[test]
fn asking_again_replaces_the_previous_answer() {
    let document = queue_doc("- do [#alpha]\n  - status?\n");
    let once = answer_status_queries(&document).unwrap().unwrap().content;
    let struck = once.replace("- do [#alpha]", "- ~~do [#alpha]~~");
    let asked = struck.replace(
        "<!-- /agent:queue -->",
        "  - status?\n<!-- /agent:queue -->",
    );
    let twice = answer_status_queries(&asked).unwrap().unwrap().content;
    assert_eq!(twice.matches("**Status:**").count(), 1, "{twice}");
    assert!(
        twice.contains("**Deletable:** “do `#alpha`”"),
        "the new answer reflects the struck row: {twice}"
    );
    assert!(!twice.contains("**Next: agent:**"), "{twice}");
}

#[test]
fn a_token_on_the_row_answers_as_its_first_child() {
    let document = queue_doc("- do [#alpha] [status?]\n  - do [#alpha-child]\n");
    let answered = answer_status_queries(&document).unwrap().unwrap().content;
    assert_eq!(
        answered,
        queue_doc(concat!(
            "- do [#alpha]\n",
            "  - **Status:** snapshot — 0 you · 2 agent · 0 deletable · 0 need review ▫️\n",
            "    - **Next: agent:** “do `#alpha`” — pick up this queued item ▫️\n",
            "    - **Next: agent:** “do `#alpha-child`” — pick up this queued item ▫️\n",
            "  - do [#alpha-child]\n",
        ))
    );
    assert_eq!(prompts(&answered), vec!["do [#alpha-child]".to_string()]);
    // A leaf keeps its prompt with Status rows beneath it.
    let leaf = queue_doc("- do [#alpha] [status?]\n- do [#beta]\n");
    let answered = answer_status_queries(&leaf).unwrap().unwrap().content;
    assert!(
        answered.starts_with("<!-- agent:queue id=\"main\" -->\n- do [#alpha]\n  - **Status:**"),
        "{answered}"
    );
    assert_eq!(
        prompts(&answered),
        vec!["do [#alpha]".to_string(), "do [#beta]".to_string()]
    );
}

#[test]
fn a_top_level_query_answers_for_the_whole_queue() {
    let document = queue_doc("- do [#alpha]\n- ~~do [#beta]~~\n- status?\n");
    let answered = answer_status_queries(&document).unwrap().unwrap();
    assert_eq!(answered.answered[0].target, "queue main");
    assert!(
        answered.content.contains(
            "- **Status:** snapshot — 0 you · 1 agent · 1 deletable · 0 need review ▫️\n"
        )
    );
    assert_eq!(prompts(&answered.content), vec!["do [#alpha]".to_string()]);
}

#[test]
fn only_a_deep_query_with_review_rows_queues_an_agent_turn() {
    let deep = queue_doc("- do [#alpha]\n  - ⏭️ do [#skipped]\n  - status? deep\n");
    let answered = answer_status_queries(&deep).unwrap().unwrap();
    assert!(answered.answered[0].queued_agent_turn);
    assert!(
        answered
            .content
            .contains("**Needs agent review:** “⏭️ do `#skipped`”")
            || answered
                .content
                .contains("**Needs agent review:** “do `#skipped`”"),
        "{}",
        answered.content
    );
    assert!(
        answered
            .content
            .contains(&format!("  - {DEEP_REVIEW_PROMPT}\n"))
    );
    assert!(
        prompts(&answered.content)
            .iter()
            .any(|text| text == DEEP_REVIEW_PROMPT)
    );

    let plain = queue_doc("- do [#alpha]\n  - ⏭️ do [#skipped]\n  - status?\n");
    let answered = answer_status_queries(&plain).unwrap().unwrap();
    assert!(!answered.answered[0].queued_agent_turn);
    assert!(!answered.content.contains(DEEP_REVIEW_PROMPT));

    let nothing = queue_doc("- do [#alpha]\n  - status? deep\n");
    let answered = answer_status_queries(&nothing).unwrap().unwrap();
    assert!(!answered.answered[0].queued_agent_turn);
    assert!(
        answered.content.contains("no agent turn was queued"),
        "{}",
        answered.content
    );
}

#[test]
fn a_query_inside_a_waiting_packet_is_never_an_answer() {
    let body = concat!(
        "- [#picked] Pick a port. <!-- agent-doc:waiting-item -->\n",
        "  - **Response:** Pick a port. ▫️\n",
        "  - status?\n",
    );
    let document = format!(
        "{}<!-- agent:todo -->\n- [ ] [#picked] Pick a port.\n<!-- /agent:todo -->\n",
        queue_doc(body)
    );
    let component = element::parse(&document)
        .unwrap()
        .into_iter()
        .find(|component| component.name == "queue")
        .unwrap();
    let entries = document_queue::parse(component.content(&document)).unwrap();
    let open = crate::queue_consume::open_waiting_todo_ids(&document);
    let status = waiting_status::waiting_status(&entries, 0, &open).unwrap();
    assert_eq!(
        status.state,
        waiting_status::WaitingState::Operator,
        "{status:#?}"
    );
    let answered = answer_status_queries(&document).unwrap().unwrap();
    assert!(
        answered.content.contains("**Next: you:** “"),
        "{}",
        answered.content
    );
}

#[test]
fn the_snapshot_carries_an_answered_query_but_not_other_edits() {
    let snapshot = queue_doc("- do [#alpha]\n");
    let before = queue_doc("- do [#alpha]\n  - status?\n");
    let after = answer_status_queries(&before).unwrap().unwrap().content;
    let carried = carry_status_answers_into_snapshot(&snapshot, &before, &after).unwrap();
    assert_eq!(carried, after);

    // Another unsnapshotted queue edit keeps the whole component as a diff.
    let edited = queue_doc("- do [#alpha]\n  - status?\n- new operator work\n");
    let after = answer_status_queries(&edited).unwrap().unwrap().content;
    assert!(carry_status_answers_into_snapshot(&snapshot, &edited, &after).is_none());

    // A deep query that queued a turn stays visible as a diff.
    let deep = queue_doc("- do [#alpha]\n  - ⏭️ do [#s]\n  - status? deep\n");
    let deep_snapshot = queue_doc("- do [#alpha]\n  - ⏭️ do [#s]\n");
    let after = answer_status_queries(&deep).unwrap().unwrap().content;
    assert!(carry_status_answers_into_snapshot(&deep_snapshot, &deep, &after).is_none());
}

#[test]
fn status_rows_carry_no_id_references() {
    assert_eq!(
        sanitize("~~do [#alpha]~~ and [#beta]"),
        "do `#alpha` and `#beta`"
    );
}

/// The example in docs/guide/queue-markers.md ("Asking a subtree's status").
#[test]
fn the_guide_example_is_what_preflight_writes() {
    let document = queue_doc(concat!(
        "- ~~do [#release]~~\n",
        "  - 🙋 Approve the changelog\n",
        "  - do [#publish]\n",
        "  - ~~do [#build]~~\n",
        "  - status?\n",
    ));
    let answered = answer_status_queries(&document).unwrap().unwrap().content;
    let guide = include_str!("../../docs/guide/queue-markers.md");
    let start = guide.find("- ~~do [#release]~~").unwrap();
    let end = start + guide[start..].find("```").unwrap();
    assert_eq!(answered, queue_doc(&guide[start..end]), "{answered}");
}

#[test]
fn a_query_the_typing_gate_holds_waits() {
    let document = queue_doc("- do [#alpha]\n  - status?\n- do [#beta] [status?]\n");
    let held = answer_status_queries_settled(&document, &|_| false).unwrap();
    assert!(held.is_none(), "nothing settled, nothing answered");
    let only_token = answer_status_queries_settled(&document, &|text| text.contains("[status?]"))
        .unwrap()
        .unwrap();
    assert_eq!(only_token.answered.len(), 1);
    assert!(
        only_token.content.contains("  - status?\n"),
        "{}",
        only_token.content
    );
    assert!(
        !only_token.content.contains("[status?]"),
        "{}",
        only_token.content
    );
}
