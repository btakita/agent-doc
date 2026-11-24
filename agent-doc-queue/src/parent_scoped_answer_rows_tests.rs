//! `#parentscopedanswer`: identical short answer rows (`#justdoit`) under
//! different parents never share a response, a claim, or a position.
//!
//! The shape is the step2 tree of `tasks/agent-doc/agent-doc-bugs.md` on
//! 2026-11-23: two Remaining entries, the operator answering `#justdoit1p`
//! under the first and `#justdoit` under the second, while an older
//! `#justdoit` row under the VS Code tree had its own `subagent:justdoit`
//! hand-back (`[#r-6837aa93963f]`).

use crate::document_queue::{self, QueueEntry};
use crate::queue_claim::{ClaimQueueView, QueueClaimLedger, claim_identity};

const STEP2_ROOT: &str = "[#q-e9483473162d] follow-up: agent: step 2 of the operator direction is not started. [#q-1946605401b3]";
const MOVE_ENTRY: &str = "agent: move ownership of the per-document background maintenance request to the project controller";
const COMMIT_ENTRY: &str = "agent: make the commit-time queue continuation reconcile";
const OLD_ROOT: &str = "follow up on an action item from queue item “complete the work for this tree”: operator: decide whether to refresh the VS Code package. [operator-verify] [#q-cb3ac1276fd3]";
const VSCODE_SUMMARY: &str = "I refreshed the agent-doc VS Code package.";
const COMMIT_SUMMARY: &str =
    "Done by step 3 (1dcabe93cc): the commit-time reconcile is incremental.";
const AT: &str = "2026-11-23T09:26:36.000Z";

fn step2_tree(first_answer: &str, second_answer: &str) -> String {
    format!(
        "- ### {STEP2_ROOT}\n\
         \x20 - **Response [#r-a4794e8fee5c]**\n\
         \x20   - **Timestamp:** 2026-11-23T08:12:40.000Z\n\
         \x20   - **Summary:** The reconcile writes are off the preflight hook.\n\
         \x20   - **Action items:** <!-- agent-doc:response-detail -->\n\
         \x20     - {MOVE_ENTRY}. Preflight still spawns a separate worker process. <!-- agent-doc:response-detail -->\n\
         \x20     - {COMMIT_ENTRY} targeted. It costs 2.5–3.3s inside every commit. <!-- agent-doc:response-detail -->\n\
         \x20   - **Remaining:**\n\
         \x20     - ⏳ {MOVE_ENTRY} <!-- agent-doc:response-detail -->\n\
         \x20       - {first_answer}\n\
         \x20     - ⏳ {COMMIT_ENTRY} <!-- agent-doc:response-detail -->\n\
         \x20       - {second_answer}\n\
         \x20   - **State:** Follow-up work remains.\n\
         \x20   - **Guidance:** ⏳ 1/3 done. 2 items remain, listed under Remaining. Keep this tree until they are done.\n"
    )
}

fn vscode_cell() -> String {
    let handback = format!(
        "**Summary**\n\n{VSCODE_SUMMARY}\n\n\
         **What changed**\n\n- Installed the VSIX into VSCodium.\n\n\
         **Verification**\n\n- `codium --list-extensions` lists it.\n\n\
         **Action items**\n\n- operator: reload any open VSCodium windows.\n\n\
         **Notes**\n\nnone\n\n\
         **Next actions**\n\n- none\n"
    );
    agent_doc_document::subagent_completion::cell_with_response(
        "subagent:justdoit",
        &format!("{OLD_ROOT}: #justdoit"),
        "justdoit-key",
        &handback,
    )
    .unwrap()
}

/// The live document. `old_rows` is the older VS Code tree (empty once the
/// operator deleted it); its `subagent:justdoit` completion cell stays in the
/// exchange either way.
fn doc(old_rows: &str, first_answer: &str, second_answer: &str) -> String {
    format!(
        "---\nqueue_active: true\n---\n\n\
         <!-- agent:exchange -->\n{}\n<!-- /agent:exchange -->\n\n\
         <!-- agent:run id=\"main\" subagents -->\n\
         {old_rows}{}\
         - do [#install]\n\
         <!-- /agent:run -->\n",
        vscode_cell(),
        step2_tree(first_answer, second_answer),
    )
}

fn old_tree_with_response() -> String {
    format!(
        "- ### {OLD_ROOT}\n\
         \x20 - ~~#justdoit~~\n\
         \x20   - **Response [#r-6837aa93963f]**\n\
         \x20     - **Timestamp:** 2026-11-23T04:07:09.000Z\n\
         \x20     - **Summary:** {VSCODE_SUMMARY}\n\
         \x20     - **State:** Complete.\n\
         \x20     - **Guidance:** ✅ Done.\n"
    )
}

/// The step2 tree's lines, from its root to the next top-level row.
fn step2_lines(document: &str) -> Vec<String> {
    let start = document
        .find(&format!("- ### {STEP2_ROOT}"))
        .expect("step2 root");
    document[start..]
        .lines()
        .enumerate()
        .take_while(|(index, line)| *index == 0 || line.starts_with(' '))
        .map(|(_, line)| line.to_string())
        .collect()
}

fn queue_entries(document: &str) -> Vec<QueueEntry> {
    let components = agent_doc_element::element::parse(document).unwrap();
    let queue = components
        .iter()
        .find(|component| component.name == "queue")
        .expect("queue");
    document_queue::parse(queue.content(document)).unwrap()
}

/// Claim lineage of every prompt row whose own text is `text`.
fn lineages_of(document: &str, text: &str) -> Vec<String> {
    let entries = queue_entries(document);
    entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| match entry {
            QueueEntry::Prompt(prompt) | QueueEntry::Completed(prompt)
                if prompt.text.trim() == text =>
            {
                document_queue::lineage_claim_text(&entries, index)
            }
            _ => None,
        })
        .collect()
}

/// Strike `#justdoit` under the commit-time entry the way `queue consume
/// --ack-text '#justdoit' --summary …` does: the stopped-row/hand-back pass,
/// then the ack summary.
fn strike_commit_answer(before: &str) -> String {
    let after = before.replacen(
        "      - ⏳ agent: make the commit-time queue continuation reconcile <!-- agent-doc:response-detail -->\n        - #justdoit\n",
        "      - ⏳ agent: make the commit-time queue continuation reconcile <!-- agent-doc:response-detail -->\n        - ~~#justdoit~~\n",
        1,
    );
    assert_ne!(after, before, "the commit-time answer is struck");
    let attached =
        crate::queue_consume::attach_stopped_row_responses(before, &after, "", AT).unwrap();
    crate::queue_consume::attach_ack_summary_response(before, &attached, Some(COMMIT_SUMMARY), AT)
        .unwrap()
        .document
}

#[test]
fn an_answer_under_a_remaining_entry_carries_the_entry_in_its_lineage() {
    let live = doc("", "#justdoit", "#justdoit");
    let lineages = lineages_of(&live, "#justdoit");
    assert_eq!(
        lineages,
        vec![
            format!("{STEP2_ROOT}: {MOVE_ENTRY}: #justdoit"),
            format!("{STEP2_ROOT}: {COMMIT_ENTRY}: #justdoit"),
        ],
        "the two identical answers are two rows"
    );
    assert_ne!(claim_identity(&lineages[0]), claim_identity(&lineages[1]));

    // The entry's status glyph is progress, not identity.
    let done = live.replacen(
        &format!("- ⏳ {COMMIT_ENTRY}"),
        &format!("- ✅ {COMMIT_ENTRY}"),
        1,
    );
    assert_eq!(lineages_of(&done, "#justdoit"), lineages);
}

#[test]
fn striking_one_justdoit_never_takes_an_older_rows_handback() {
    // The live shape: the old VS Code row was deleted, its cell stayed.
    for old_rows in [String::new(), old_tree_with_response()] {
        let live = doc(&old_rows, "#justdoit1p", "#justdoit");
        let struck = strike_commit_answer(&live);
        let tree = step2_lines(&struck).join("\n");
        assert!(
            !tree.contains(VSCODE_SUMMARY) && !tree.contains("[#r-6837aa93963f]"),
            "the VS Code hand-back stays with its own row:\n{tree}"
        );
        assert!(
            tree.contains(&format!("**Summary:** {COMMIT_SUMMARY}")),
            "the strike carries the coordinator's summary:\n{tree}"
        );
        // No answer row moved: `#justdoit1p` is still the first entry's answer
        // and nothing was added under the tree root.
        let lines = step2_lines(&struck);
        let first_entry = lines
            .iter()
            .position(|line| line.contains(&format!("⏳ {MOVE_ENTRY}")))
            .unwrap();
        assert_eq!(lines[first_entry + 1], "        - #justdoit1p");
        assert!(
            !lines.iter().any(|line| line.starts_with("  - #justdoit")),
            "no answer row hangs off the tree root:\n{tree}"
        );
        if !old_rows.is_empty() {
            // Nothing before the old tree moved. The exchange gained only the
            // strike's queue-response record (`#exchangeresponseid`).
            let from_close = |d: &str| d[d.find("<!-- /agent:exchange -->").unwrap()..].to_string();
            let live_tail = from_close(&live);
            assert!(
                from_close(&struck).starts_with(&live_tail[..live_tail.find(&old_rows).unwrap()])
            );
            assert_eq!(
                agent_doc_document::queue_response_record::record_heading_lines(&struck).len(),
                1,
                "{struck}"
            );
            assert!(struck.starts_with(&live[..live.find("<!-- /agent:exchange -->").unwrap()]));
            let old_start = struck.find(&format!("- ### {OLD_ROOT}")).unwrap();
            let old_tree = &struck[old_start..struck.find(&format!("- ### {STEP2_ROOT}")).unwrap()];
            assert!(
                old_tree.starts_with(&format!(
                    "- ### {OLD_ROOT}\n  - ~~#justdoit~~\n    - **Response [#r-6837aa93963f]**\n"
                )) && old_tree.contains(VSCODE_SUMMARY),
                "the old row keeps its own tree:\n{old_tree}"
            );
        }
        assert!(
            crate::queue_consume::mismatched_row_responses(&struck).is_empty(),
            "{:?}",
            crate::queue_consume::mismatched_row_responses(&struck)
        );
    }
}

#[test]
fn identical_answers_under_two_entries_get_their_own_response_ids() {
    let live = doc(&old_tree_with_response(), "#justdoit", "#justdoit");
    let first = live.replacen("        - #justdoit\n", "        - ~~#justdoit~~\n", 1);
    let first =
        crate::queue_consume::attach_ack_summary_response(&live, &first, Some(COMMIT_SUMMARY), AT)
            .unwrap()
            .document;
    let second = first.replacen("        - #justdoit\n", "        - ~~#justdoit~~\n", 1);
    let second = crate::queue_consume::attach_ack_summary_response(
        &first,
        &second,
        Some(COMMIT_SUMMARY),
        AT,
    )
    .unwrap()
    .document;
    let ids = step2_lines(&second)
        .into_iter()
        .filter(|line| {
            line.trim_start().starts_with("- **Response [#r-") && line.starts_with("          ")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        ids.len(),
        2,
        "each struck answer holds its own tree:\n{second}"
    );
    assert_ne!(
        ids[0].trim(),
        ids[1].trim(),
        "the same answer and summary under two entries mint two ids"
    );
}

#[test]
fn session_check_reports_a_response_from_another_parents_handback() {
    // The tree as the pre-fix strike left it: the commit-time answer holds
    // the VS Code hand-back of the older `#justdoit` row.
    let wrong = format!(
        "~~#justdoit~~\n\
         \x20         - **Response [#r-6837aa93963f]**\n\
         \x20           - **Timestamp:** 2026-11-23T09:07:09.000Z\n\
         \x20           - **Summary:** {VSCODE_SUMMARY}\n\
         \x20           - **Details:** The full Summary, What changed, Verification, Notes and Next actions are in the `subagent:justdoit` hand-back in agent:exchange.\n\
         \x20           - **State:** Follow-up work remains."
    );
    let live = doc("", "#justdoit1p", &wrong);
    let reported = crate::queue_consume::mismatched_row_responses(&live);
    assert_eq!(reported.len(), 1, "{reported:?}");
    assert_eq!(reported[0].row, "~~#justdoit~~");
    assert_eq!(reported[0].response_id, "[#r-6837aa93963f]");
    assert_eq!(reported[0].owner, "subagent:justdoit");
    assert!(reported[0].reported_item.contains("[#q-cb3ac1276fd3]"));

    // The same tree under the row the cell reports is not reported.
    let own = format!(
        "- ### {OLD_ROOT}\n\
         \x20 - ~~#justdoit~~\n\
         \x20   - **Response [#r-6837aa93963f]**\n\
         \x20     - **Summary:** {VSCODE_SUMMARY}\n\
         \x20     - **Details:** The full Summary, What changed, Verification, Notes and Next actions are in the `subagent:justdoit` hand-back in agent:exchange.\n\
         \x20     - **State:** Complete.\n"
    );
    let correct = doc(&own, "#justdoit1p", "#justdoit");
    assert!(crate::queue_consume::mismatched_row_responses(&correct).is_empty());
}

fn running_claim(item: &str, owner: &str, claim_time_row: &str) -> crate::queue_claim::QueueClaim {
    serde_json::from_value(serde_json::json!({
        "identity": claim_identity(item),
        "item_text": item,
        "owner": owner,
        "generation": 1,
        "claimed_at_secs": 1,
        "expires_at_secs": 2,
        "claim_time_row": claim_time_row,
    }))
    .unwrap()
}

fn queue_body(document: &str) -> String {
    let components = agent_doc_element::element::parse(document).unwrap();
    components
        .iter()
        .find(|component| component.name == "queue")
        .unwrap()
        .content(document)
        .to_string()
}

#[test]
fn an_operator_edit_of_a_claimed_answer_is_never_restored_as_a_stray_row() {
    // The operator edited the claimed `#justdoit1p` to `#justdoit`; the row
    // keeps the attempt's owner annotation. Claims keyed both ways (with the
    // entry in the vector, and the pre-fix vector without it) leave it alone.
    let edited = doc("", "🚧 [#subagent:ctlmaint#1] #justdoit", "~~#justdoit~~");
    let body = queue_body(&edited);
    for item in [
        format!("{STEP2_ROOT}: {MOVE_ENTRY}: #justdoit1p"),
        format!("{STEP2_ROOT}: #justdoit1p"),
    ] {
        let claim = running_claim(&item, "subagent:ctlmaint", "#justdoit1p");
        assert_eq!(
            document_queue::restore_missing_nested_claim_row(&body, &claim).unwrap(),
            None,
            "{item}"
        );
    }

    // The row was deleted outright: a pre-fix claim still cannot put it back
    // under the tree root, where it was never authored.
    let deleted = doc("", "~~#justdoit~~", "~~#justdoit~~").replacen(
        &format!(
            "      - ⏳ {MOVE_ENTRY} <!-- agent-doc:response-detail -->\n        - ~~#justdoit~~\n"
        ),
        &format!("      - ⏳ {MOVE_ENTRY} <!-- agent-doc:response-detail -->\n"),
        1,
    );
    let claim = running_claim(
        &format!("{STEP2_ROOT}: #justdoit1p"),
        "subagent:ctlmaint",
        "#justdoit1p",
    );
    assert_eq!(
        document_queue::restore_missing_nested_claim_row(&queue_body(&deleted), &claim).unwrap(),
        None
    );
}

#[test]
fn a_claim_follows_the_operators_edit_of_its_answer_row() {
    let item = format!("{STEP2_ROOT}: {MOVE_ENTRY}: #justdoit1p");
    let mut ledger = QueueClaimLedger::default();
    ledger.claim(&item, "subagent:ctlmaint", 100, 3600);
    ledger
        .start(
            &item,
            "subagent:ctlmaint",
            "worker:ctlmaint",
            101,
            3600,
            Some(1),
        )
        .unwrap();
    let before = doc(
        &old_tree_with_response(),
        "🚧 [#subagent:ctlmaint#1] #justdoit1p",
        "#justdoit",
    );
    ledger.follow_edits_in_view(102, &ClaimQueueView::from_content(&before).unwrap());

    let edited = strike_commit_answer(&before).replacen(
        "🚧 [#subagent:ctlmaint#1] #justdoit1p",
        "🚧 [#subagent:ctlmaint#1] #justdoit",
        1,
    );
    ledger.follow_edits_in_view(103, &ClaimQueueView::from_content(&edited).unwrap());
    assert_eq!(ledger.claims.len(), 1);
    assert_eq!(
        ledger.claims[0].identity,
        claim_identity(&format!("{STEP2_ROOT}: {MOVE_ENTRY}: #justdoit")),
        "the claim follows the edited row, not the struck sibling"
    );
    assert_eq!(ledger.claims[0].generation, 1);
    assert_eq!(
        ledger.claimed_live_head_texts(104, &edited),
        vec![format!("{STEP2_ROOT}: {MOVE_ENTRY}: #justdoit")],
    );
}
