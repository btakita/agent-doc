//! `#pipeprompt` ` |>` hand-off pipe: structure, holds and the close
//! transition.

use super::*;
use crate::document_queue::OrderedStepWait;
use crate::document_queue::{
    entry_is_finished_hand_off_head, entry_is_group_parent, entry_is_open_hand_off_head,
    executable_prompt_indexes, ordered_step_waits, parse,
};

fn queue_doc(body: &str) -> String {
    format!("<!-- agent:run -->\n{body}<!-- /agent:run -->\n")
}

fn body_of(doc: &str) -> &str {
    doc.strip_prefix("<!-- agent:run -->\n")
        .and_then(|rest| rest.strip_suffix("<!-- /agent:run -->\n"))
        .unwrap()
}

/// Work text of the rows a drain may run now: executable and not held.
fn runnable(body: &str) -> Vec<String> {
    let entries = parse(body).unwrap();
    let waits = ordered_step_waits(&entries);
    executable_prompt_indexes(&entries)
        .into_iter()
        .filter(|index| waits[*index].is_none())
        .filter_map(|index| match &entries[index] {
            QueueEntry::Prompt(prompt) => Some(prompt.text.clone()),
            _ => None,
        })
        .collect()
}

const SEQUENTIAL: &str = "\
- #review |> [#q-0123456789ab]
  1. #address
  2. summarize the changes
- next item
";

const PARALLEL: &str = "\
- #review |> [#q-0123456789ab]
  - fix the docs
  - fix the tests
- next item
";

#[test]
fn an_open_hand_off_row_is_stage_one_and_holds_every_stage() {
    for body in [SEQUENTIAL, PARALLEL] {
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 4, "{entries:?}");
        assert!(entry_is_open_hand_off_head(&entries, 0));
        assert!(!entry_is_group_parent(&entries, 0), "{body}");
        assert_eq!(executable_prompt_indexes(&entries), vec![0, 1, 2, 3]);
        let waits = ordered_step_waits(&entries);
        for stage in [1, 2] {
            let wait = waits[stage].as_ref().expect("stage waits on stage 1");
            assert!(wait.hand_off);
            assert_eq!(wait.step_entry, 0);
            assert_eq!(wait.citation(), "stage-1");
        }
        assert_eq!(
            runnable(body),
            vec!["#review |> [#q-0123456789ab]", "next item"]
        );
    }
}

#[test]
fn a_finished_head_releases_numbered_stages_in_order_and_bulleted_stages_together() {
    let sequential = SEQUENTIAL.replace("- #review |>", "- ✅ #review |>");
    let entries = parse(&sequential).unwrap();
    assert!(entry_is_finished_hand_off_head(&entries, 0));
    assert!(entry_is_group_parent(&entries, 0));
    assert_eq!(runnable(&sequential), vec!["#address", "next item"]);
    let waits = ordered_step_waits(&entries);
    assert_eq!(waits[2].as_ref().unwrap().citation(), "step-1");

    let address_done = sequential.replace("1. #address", "1. ~~#address~~");
    assert_eq!(
        runnable(&address_done),
        vec!["summarize the changes", "next item"]
    );

    let parallel = PARALLEL.replace("- #review |>", "- ✅ #review |>");
    assert_eq!(
        runnable(&parallel),
        vec!["fix the docs", "fix the tests", "next item"]
    );
}

#[test]
fn the_done_glyph_is_not_part_of_any_identity() {
    let open = parse(SEQUENTIAL).unwrap();
    let done = parse(&SEQUENTIAL.replace("- #review |>", "- ✅ #review |>")).unwrap();
    // A stage keeps its claim identity across the head's transition.
    for stage in [1, 2] {
        assert_eq!(
            crate::queue_claim::claim_identity_at(&open, stage),
            crate::queue_claim::claim_identity_at(&done, stage),
        );
        assert_eq!(
            crate::document_queue::lineage_claim_text(&open, stage),
            crate::document_queue::lineage_claim_text(&done, stage),
        );
    }
    // Toggling ` |>` keeps the head's and its stages' identity.
    let plain = parse(&SEQUENTIAL.replace(" |>", "")).unwrap();
    assert_eq!(
        crate::queue_claim::claim_identity_at(&open, 0),
        crate::queue_claim::claim_identity_at(&plain, 0),
    );
}

#[test]
fn a_struck_head_with_open_stages_becomes_done_and_keeps_its_response() {
    let struck = queue_doc(concat!(
        "- ### ~~#review |> [#q-0123456789ab]~~\n",
        "  1. #address\n",
        "  2. summarize the changes\n",
        "  - **Response [#r-0123456789ab]**\n",
        "    - **Summary:** reviewed\n",
        "- next item\n",
    ));
    let (settled, report) = settle_hand_off_heads_with_report(&struck).expect("settles");
    assert_eq!(
        settled,
        queue_doc(concat!(
            "- ### ✅ #review |> [#q-0123456789ab]\n",
            "  1. #address\n",
            "  2. summarize the changes\n",
            "  - **Response [#r-0123456789ab]**\n",
            "    - **Summary:** reviewed\n",
            "- next item\n",
        ))
    );
    assert_eq!(
        report,
        vec![HandOffSettle {
            row: "#review |> [#q-0123456789ab]".to_string(),
            stage_done: true,
        }]
    );
    assert_eq!(settle_hand_off_heads(&settled), None, "idempotent");
    assert_eq!(runnable(body_of(&settled)), vec!["#address", "next item"]);
}

#[test]
fn the_head_is_struck_with_its_last_stage() {
    let done = queue_doc(concat!(
        "- ### ✅ #review |> [#q-0123456789ab]\n",
        "  1. ~~#address~~\n",
        "  2. ~~summarize the changes~~\n",
        "  - **Response [#r-0123456789ab]**\n",
        "    - **Summary:** reviewed\n",
        "- next item\n",
    ));
    let settled = settle_hand_off_heads(&done).expect("strikes the head");
    assert!(
        settled.starts_with(
            "<!-- agent:run -->\n- ### ~~#review |> [#q-0123456789ab]~~\n  1. ~~#address~~\n"
        ),
        "{settled}"
    );
    assert_eq!(settle_hand_off_heads(&settled), None);
    // A struck head whose stages are all closed stays struck.
    let closed = queue_doc("- ~~#review |>~~\n  - ~~fix~~\n");
    assert_eq!(settle_hand_off_heads(&closed), None);
    // A `✅` head with an open stage stays.
    let open = queue_doc("- ✅ #review |>\n  - ~~fix a~~\n  - fix b\n");
    assert_eq!(settle_hand_off_heads(&open), None);
    // A nested numbered head, with a strike annotation the transition drops.
    let nested = queue_doc(
        "- epic\n  1. ~~#review |>~~ — auto-struck: answered this cycle (#ftstrike)\n     - fix\n  2. ship\n",
    );
    assert_eq!(
        settle_hand_off_heads(&nested).unwrap(),
        queue_doc("- epic\n  1. ✅ #review |>\n     - fix\n  2. ship\n")
    );
    // A nested ` |>` struck with its last stage finishes the outer stage too.
    let chained = queue_doc(
        "- ✅ review |>\n  1. ~~address~~\n  2. ✅ summarize |>\n     - ~~post a~~\n     - ~~post b~~\n",
    );
    assert_eq!(
        settle_hand_off_heads(&chained).unwrap(),
        queue_doc(
            "- ~~review |>~~\n  1. ~~address~~\n  2. ~~summarize |>~~\n     - ~~post a~~\n     - ~~post b~~\n"
        )
    );
    // Not a runnable component: untouched.
    assert_eq!(settle_hand_off_heads("- ~~#review |>~~\n  - fix\n"), None);
}

#[test]
fn a_hand_off_row_is_never_folded() {
    let nodes =
        agent_doc_markdown_ast::mutations::item_nodes(&queue_doc(SEQUENTIAL), "queue").unwrap();
    assert_eq!(nodes.len(), 4);
    let entries = parse(SEQUENTIAL).unwrap();
    assert!(!crate::document_queue::entry_is_edge_block(&entries, 0));
    // Striking the head strikes only the head row.
    let struck = agent_doc_markdown_ast::mutations::consume_node(
        &queue_doc(SEQUENTIAL),
        "queue",
        &nodes[0].node_key,
    )
    .unwrap();
    assert!(
        struck.contains("~~#review |> [#q-0123456789ab]~~\n  1. #address\n  2. summarize"),
        "{struck}"
    );
}

/// The close transition over the real strike and response attachment: the
/// head ends `✅` with its response tree, never struck, and the same steps on
/// the last stage strike the head in that write.
#[test]
fn ack_strike_then_settle_closes_stage_one_with_its_response() {
    let doc = queue_doc(SEQUENTIAL);
    let nodes = agent_doc_markdown_ast::mutations::item_nodes(&doc, "queue").unwrap();
    let struck =
        crate::queue_consume::consume_queue_nodes_by_key(&doc, &[nodes[0].node_key.clone()])
            .unwrap();
    let attached = crate::queue_consume::attach_ack_summary_response(
        &doc,
        &struck,
        Some("Reviewed the PR; two findings."),
        "2026-11-24T00:00:00.000Z",
    )
    .unwrap();
    assert!(attached.bare_rows.is_empty(), "{:?}", attached.bare_rows);
    let settled = settle_hand_off_heads(&attached.document).expect("stage 1 closes");
    let body = body_of(&settled);
    assert!(
        body.starts_with("- ### ✅ #review |> [#q-0123456789ab]\n"),
        "{body}"
    );
    assert!(
        body.contains("**Summary:** Reviewed the PR; two findings."),
        "{body}"
    );
    assert!(!body.contains("~~#review"), "{body}");
    assert_eq!(runnable(body), vec!["#address", "next item"]);

    // Stage 2 and stage 3 close; the last one strikes the head too.
    let mut current = settled;
    for stage in ["#address", "summarize the changes"] {
        let nodes = agent_doc_markdown_ast::mutations::item_nodes(&current, "queue").unwrap();
        let key = nodes
            .iter()
            .find(|node| node.item.text.trim().ends_with(stage) && !node.item.struck)
            .unwrap_or_else(|| panic!("{stage} in {current}"))
            .node_key
            .clone();
        let struck = crate::queue_consume::consume_queue_nodes_by_key(&current, &[key]).unwrap();
        let attached = crate::queue_consume::attach_ack_summary_response(
            &current,
            &struck,
            Some("done"),
            "2026-11-24T00:00:00.000Z",
        )
        .unwrap();
        current = settle_hand_off_heads(&attached.document).unwrap_or(attached.document);
    }
    let body = body_of(&current);
    assert!(
        body.starts_with("- ### ~~#review |> [#q-0123456789ab]~~\n"),
        "{body}"
    );
    assert_eq!(runnable(body), vec!["next item"]);
}

/// A session document after stage 1 closed: `✅` head, its response tree
/// and the exchange entry carrying the response id.
fn after_stage_one(stages: &str) -> String {
    let exchange = concat!(
        "<!-- agent:exchange -->\n",
        "### Re: Queue response: #review — agent-doc [#r-0123456789ab]\n",
        "> **Subagent response:**\n",
        "> **Summary** Found two issues: the parser drops `N)` and the lint misses ` |>`.\n",
        "> **What changed** none\n",
        "\n",
        "### Re: something else — agent-doc [#r-fedcba987654]\n",
        "unrelated\n",
        "<!-- /agent:exchange -->\n",
    );
    let response = concat!(
        "  - **Response [#r-0123456789ab]**\n",
        "    - **Summary:** Found two issues.\n",
        "    - **State:** Complete.\n",
    );
    format!(
        "{exchange}<!-- agent:run -->\n- ### ✅ #review |> [#q-0123456789ab]\n{stages}{response}- next item\n<!-- /agent:run -->\n"
    )
}

#[test]
fn a_stage_brief_carries_the_previous_stage_hand_back_from_the_exchange() {
    let content = after_stage_one("  1. #address\n  2. summarize the changes\n");
    let input = hand_off_input_for_item(&content, "#address").expect("stage 2 input");
    assert_eq!(input.from_stage, "#review");
    assert_eq!(input.hand_backs.len(), 1);
    let hand_back = &input.hand_backs[0];
    assert_eq!(hand_back.response_id.as_deref(), Some("[#r-0123456789ab]"));
    assert!(hand_back.from_exchange);
    assert!(
        hand_back.text.contains("the parser drops `N)`"),
        "{}",
        hand_back.text
    );
    assert!(!hand_back.text.contains("unrelated"), "{}", hand_back.text);

    let auth = crate::subagent_brief::subagent_authorization("doc.md", &content, "#address");
    assert!(
        auth.subagent_prompt_preamble.contains(
            "Hand-off input (`#pipeprompt`, ` |>`): the hand-back of the previous stage, `#review`."
        ),
        "{}",
        auth.subagent_prompt_preamble
    );
    assert!(
        auth.subagent_prompt_preamble
            .contains("> > **Summary** Found two issues: the parser drops `N)`"),
        "{}",
        auth.subagent_prompt_preamble
    );
    assert!(
        auth.subagent_prompt_preamble
            .contains(HAND_OFF_STAGE_HEADER_PROBE),
        "{}",
        auth.subagent_prompt_preamble
    );
    assert_eq!(auth.hand_off_input, Some(input));
}

const HAND_OFF_STAGE_HEADER_PROBE: &str = "Hand-off stage (`#pipeprompt`, ` |>`)";

#[test]
fn a_later_numbered_stage_takes_the_stage_before_it_and_bulleted_stages_share_stage_one() {
    let content = after_stage_one(concat!(
        "  1. ~~#address~~\n",
        "     - **Response [#r-aaaaaaaaaaaa]**\n",
        "       - **Summary:** Fixed both issues in 1a2b3c.\n",
        "  2. summarize the changes\n",
    ));
    let input = hand_off_input_for_item(&content, "summarize the changes").unwrap();
    assert_eq!(input.from_stage, "#address");
    assert_eq!(input.hand_backs.len(), 1);
    // No exchange entry carries that id: the response tree is the input.
    assert!(!input.hand_backs[0].from_exchange);
    assert!(
        input.hand_backs[0].text.starts_with(
            "- **Response [#r-aaaaaaaaaaaa]**\n  - **Summary:** Fixed both issues in 1a2b3c."
        ),
        "{}",
        input.hand_backs[0].text
    );

    let parallel = after_stage_one("  - fix the docs\n  - fix the tests\n");
    for stage in ["fix the docs", "fix the tests"] {
        let input = hand_off_input_for_item(&parallel, stage).unwrap();
        assert_eq!(input.from_stage, "#review", "{stage}");
        assert_eq!(
            input.hand_backs[0].response_id.as_deref(),
            Some("[#r-0123456789ab]")
        );
    }
    // A row outside any hand-off has no input.
    assert_eq!(hand_off_input_for_item(&parallel, "next item"), None);
}

/// `#pipeprompt` reliability: editing a future stage changes its input. The
/// brief is built from the current document, so the edited stage is the one
/// that runs, with the same hand-back.
#[test]
fn editing_a_future_stage_changes_what_the_next_worker_gets() {
    let held = SEQUENTIAL;
    let edited = held.replace(
        "2. summarize the changes",
        "2. summarize the changes in 3 bullets",
    );
    let entries = parse(&edited).unwrap();
    let submitted = crate::document_queue::submitted_prompt_at(&entries, 2).unwrap();
    assert!(
        submitted.ends_with("\n- #review |> summarize the changes in 3 bullets"),
        "{submitted}"
    );
    let content = after_stage_one(
        "  1. ~~#address~~\n     - **Response [#r-aaaaaaaaaaaa]**\n       - **Summary:** fixed\n  2. summarize the changes in 3 bullets\n",
    );
    let auth = crate::subagent_brief::subagent_authorization(
        "doc.md",
        &content,
        "summarize the changes in 3 bullets",
    );
    assert!(
        auth.subagent_prompt_preamble
            .contains("summarize the changes in 3 bullets"),
        "{}",
        auth.subagent_prompt_preamble
    );
    assert!(
        auth.subagent_prompt_preamble.contains("**Summary:** fixed"),
        "{}",
        auth.subagent_prompt_preamble
    );
}

#[test]
fn stage_one_submission_names_its_later_stages() {
    let entries = parse(SEQUENTIAL).unwrap();
    let submitted = crate::document_queue::submitted_prompt_at(&entries, 0).unwrap();
    assert!(
        submitted.starts_with("#review |> [#q-0123456789ab]\n"),
        "{submitted}"
    );
    assert!(
        submitted.contains(crate::document_queue::HAND_OFF_PROMPT_HEADER),
        "{submitted}"
    );
    assert!(
        submitted.ends_with("\n- #review |> 1. #address\n- #review |> 2. summarize the changes"),
        "{submitted}"
    );
    // Once stage 1 finished the head is a container, never submitted again.
    let done = parse(&SEQUENTIAL.replace("- #review |>", "- ✅ #review |>")).unwrap();
    assert_eq!(crate::document_queue::submitted_prompt_at(&done, 0), None);
}

// ---------------------------------------------------------------------------
// Reliability (`#pipeprompt` phase 2). Each test pins one requirement.
// ---------------------------------------------------------------------------

/// Crash between stages, after stage 1 closed: the document alone says where
/// the pipe is. A fresh parse never re-offers stage 1 and offers stage 2.
#[test]
fn restart_after_stage_one_resumes_at_stage_two() {
    let content = after_stage_one("  1. #address\n  2. summarize the changes\n");
    let components = element::parse(&content).unwrap();
    let queue = components.iter().find(|c| c.name == "queue").unwrap();
    let body = queue.content(&content);
    assert_eq!(runnable(body), vec!["#address", "next item"]);
    // Re-running every normalizer is a no-op: nothing reopens stage 1.
    assert_eq!(settle_hand_off_heads(&content), None);
    assert_eq!(
        crate::self_repair::self_repair_document(&content, &|_, _| true, &|_| true)
            .map(|(_, repairs)| repairs
                .into_iter()
                .filter(|repair| repair.kind == crate::self_repair::SelfRepairKind::HandOffStage)
                .count())
            .unwrap_or(0),
        0
    );
}

/// Crash mid-transition: an older writer struck the head and stopped before
/// the `✅` form. Self-repair and the runtime projection both finish the
/// transition; neither reopens stage 1 as runnable work.
#[test]
fn a_head_struck_mid_transition_is_settled_never_rerun() {
    let struck = queue_doc(
        "- ### ~~#review |> [#q-0123456789ab]~~\n  1. #address\n  2. summarize the changes\n- next item\n",
    );
    let (repaired, repairs) =
        crate::self_repair::self_repair_document(&struck, &|_, _| true, &|_| true)
            .expect("self-repair settles the head");
    assert!(
        repairs
            .iter()
            .any(|repair| repair.kind == crate::self_repair::SelfRepairKind::HandOffStage),
        "{repairs:?}"
    );
    assert!(
        repaired.contains("- ### ✅ #review |> [#q-0123456789ab]\n"),
        "{repaired}"
    );
    assert_eq!(runnable(body_of(&repaired)), vec!["#address", "next item"]);

    // The lifecycle projection derives a container's state from its leaves:
    // a struck ` |>` row with a live stage is the `✅` form, never the open,
    // runnable one.
    let (projected, _) =
        crate::document_queue::project_prompt_runtime_projections(body_of(&struck), &[])
            .unwrap()
            .expect("the struck container is re-derived");
    assert!(
        projected.starts_with("- ### ✅ #review |> [#q-0123456789ab]\n"),
        "{projected}"
    );
    assert_eq!(runnable(&projected), vec!["#address", "next item"]);
    // A `✅` row whose stages all closed is derived struck.
    let finished = "- ### ✅ #review |> [#q-0123456789ab]\n  1. ~~#address~~\n  2. ~~summarize the changes~~\n- next item\n";
    let (projected, _) = crate::document_queue::project_prompt_runtime_projections(finished, &[])
        .unwrap()
        .expect("derived struck");
    assert!(
        projected.starts_with("- ### ~~#review |> [#q-0123456789ab]~~\n"),
        "{projected}"
    );
}

/// Crash after stage 1 reported but before its closeout: the stages stay
/// held, so nothing runs ahead of the hand-back.
#[test]
fn a_reported_stage_one_still_holds_its_stages() {
    for marker in ["☑️ [#subagent:review#1]", "🚧 [#subagent:review#1]"] {
        let body = format!(
            "- ### {marker} #review |> [#q-0123456789ab]\n  1. #address\n  2. summarize the changes\n  - **Response [#r-0123456789ab]**\n    - **Summary:** reviewed\n"
        );
        let entries = parse(&body).unwrap();
        let waits = ordered_step_waits(&entries);
        assert!(
            waits[1].as_ref().is_some_and(|wait| wait.hand_off),
            "{body}"
        );
        assert!(
            waits[2].as_ref().is_some_and(|wait| wait.hand_off),
            "{body}"
        );
        assert!(!runnable(&body).iter().any(|row| row.contains("#address")));
    }
}

/// A lost or frozen stage worker retries only that stage: the finished head
/// is not runnable again and the later stage still waits.
#[test]
fn a_lost_stage_retries_only_that_stage() {
    use agent_doc_document::queue_projection::QueueLifecycleMarker;
    for marker in [
        QueueLifecycleMarker::Lost,
        QueueLifecycleMarker::LostRunning,
        QueueLifecycleMarker::Running,
    ] {
        let body = format!(
            "- ### ✅ #review |> [#q-0123456789ab]\n  1. {} [#subagent:address#2] #address\n  2. summarize the changes\n  - **Response [#r-0123456789ab]**\n    - **Summary:** reviewed\n- next item\n",
            marker.prefix()
        );
        let entries = parse(&body).unwrap();
        let executable = executable_prompt_indexes(&entries);
        assert!(!executable.contains(&0), "{body}");
        assert!(executable.contains(&1), "{body}");
        let waits = ordered_step_waits(&entries);
        assert!(waits[1].is_none(), "{body}");
        assert_eq!(
            waits[2].as_ref().map(OrderedStepWait::citation).as_deref(),
            Some("step-1"),
            "{body}"
        );
    }
}

/// Editing the running stage forwards to its owner: the claim follows the
/// edited line (`#claimfollowedit`), and the head's claim is not touched.
#[test]
fn a_running_stage_claim_follows_an_edit_of_its_row() {
    let heads = |body: &str| -> Vec<String> {
        let entries = parse(body).unwrap();
        executable_prompt_indexes(&entries)
            .into_iter()
            .filter_map(|index| crate::document_queue::lineage_claim_text(&entries, index))
            .collect()
    };
    let before = "- ### ✅ #review |> [#q-0123456789ab]\n  1. address the findings\n  2. summarize the changes\n- next item\n";
    let live = heads(before);
    let stage = live
        .iter()
        .find(|head| head.ends_with("address the findings"))
        .unwrap()
        .clone();
    let mut ledger = crate::queue_claim::QueueClaimLedger::default();
    ledger.claim(&stage, "subagent:address", 100, 3600);
    ledger.follow_edits(100, &live);
    let after = before.replace(
        "1. address the findings",
        "1. address the findings, skipping the lint one",
    );
    let followed = ledger.follow_edits(110, &heads(&after));
    assert_eq!(followed.len(), 1, "{followed:?}");
    assert!(
        followed[0]
            .1
            .ends_with("address the findings, skipping the lint one")
    );
    assert_eq!(ledger.claims[0].owner, "subagent:address");
}

/// The post-commit reconciler never fights the transition: a committed
/// struck head (an interrupted write) does not re-strike the live `✅` row,
/// and the live document is a fixed point of reconcile + settle.
#[test]
fn the_postcommit_reconciler_and_the_transition_reach_a_fixed_point() {
    let done = queue_doc(
        "- ### ✅ #review |> [#q-0123456789ab]\n  1. #address\n  2. summarize\n- next item\n",
    );
    let committed_struck = queue_doc(
        "- ### ~~#review |> [#q-0123456789ab]~~\n  1. #address\n  2. summarize\n- next item\n",
    );
    for committed in [&done, &committed_struck] {
        let reconciled =
            crate::queue_consume::reconcile_postcommit_queue_strikes_to_head(&done, committed);
        let after = reconciled.clone().unwrap_or_else(|| done.clone());
        let settled = settle_hand_off_heads(&after).unwrap_or(after);
        assert_eq!(settled, done, "{committed}");
        assert_eq!(
            crate::queue_consume::reconcile_postcommit_queue_strikes_to_head(&settled, committed)
                .and_then(|again| settle_hand_off_heads(&again).or(Some(again)))
                .unwrap_or_else(|| settled.clone()),
            done
        );
    }
}

/// Stop hook: blocked future stages rest (`⛔`), so a running stage 1 is the
/// only transient row, and a finished head is not one.
#[test]
fn held_stages_are_waiting_not_transient_closeout_debt() {
    let running = queue_doc(concat!(
        "- ### 🚧 [#subagent:review#1] #review |> [#q-0123456789ab]\n",
        "  1. ⛔ ⟦blocked by stage-1⟧ #address\n",
        "  2. ⛔ ⟦blocked by stage-1⟧ summarize\n",
    ));
    let debts = crate::transient_closeout::live_transient_debts(&running).unwrap();
    assert_eq!(debts.len(), 1, "{debts:?}");
    assert!(debts[0].row.contains("#review |>"), "{debts:?}");
    let done = queue_doc(concat!(
        "- ### ✅ #review |> [#q-0123456789ab]\n",
        "  1. ⛔ ⟦blocked by stage-1⟧ #address\n",
        "  2. ⛔ ⟦blocked by step-1⟧ summarize\n",
    ));
    assert!(
        crate::transient_closeout::live_transient_debts(&done)
            .unwrap()
            .is_empty()
    );
}

/// `#parentstrike`: an explicit close of a `✅` head while a stage is open
/// names the open stage and changes nothing.
#[test]
fn a_finished_head_with_open_stages_refuses_a_parent_close() {
    let doc = queue_doc(
        "- ### ✅ #review |> [#q-0123456789ab]\n  1. #address\n  2. summarize\n  - **Response [#r-0123456789ab]**\n    - **Summary:** reviewed\n",
    );
    let start = doc.find("- ### ✅").unwrap();
    let plan = crate::followup_lift::plan_parent_close(&doc, start, &|_| false).unwrap();
    assert!(
        plan.open_rows.iter().any(|row| row.contains("#address")),
        "{:?}",
        plan.open_rows
    );
}

#[test]
fn hand_off_lints() {
    use agent_doc_markdown_ast::overlay::{PipeLintKind, pipe_statement_lints};
    let kinds = |body: &str| {
        pipe_statement_lints(&queue_doc(body))
            .into_iter()
            .map(|finding| finding.kind)
            .collect::<Vec<_>>()
    };
    assert!(kinds(SEQUENTIAL).is_empty());
    assert_eq!(
        kinds("- lonely |>\n"),
        vec![PipeLintKind::HandOffWithoutChildren]
    );
    // Inside a ` |` statement the whole block is one task.
    assert!(
        kinds("- collect |\n  - review |>\n    - address\n")
            .contains(&PipeLintKind::HandOffInsideBlock)
    );
    // A stage under ` |>` holds its own claim by design.
    assert!(kinds("- ✅ review |>\n  - 🚧 [#subagent:a#1] address\n  - post\n").is_empty());
}

#[test]
fn a_stage_scope_drops_the_earlier_stages_of_its_nest_vector() {
    assert_eq!(
        stage_scope("https://x.test/pull/12: #address |>: #adversarial-pr"),
        "#adversarial-pr"
    );
    assert_eq!(
        stage_scope("https://x.test/pull/12: #address |> [#q-0123456789ab]: #adversarial-pr"),
        "#adversarial-pr"
    );
    assert_eq!(stage_scope("a |>: b |>: c"), "c");
    // Stage 1 itself, and any row outside a hand-off, keep their whole text.
    for text in [
        "https://x.test/pull/12: #address |>",
        "#address then #adversarial-pr",
        "a |> b: c",
    ] {
        assert_eq!(stage_scope(text), text);
    }
    let presets = [
        ("#address".to_string(), "address".to_string()),
        ("#adversarial-pr".to_string(), "review".to_string()),
    ];
    let names = |text: &str| {
        crate::queue_response::queue_prompt_preset_expansions_from(
            text,
            presets.iter().map(|(name, body)| (name, body)),
        )
        .into_iter()
        .map(|(name, _)| name)
        .collect::<Vec<_>>()
    };
    assert_eq!(
        names("https://x.test/pull/12: #address |>"),
        vec!["#address"]
    );
    assert_eq!(
        names("https://x.test/pull/12: #address |>: #adversarial-pr"),
        vec!["#adversarial-pr"]
    );
}
