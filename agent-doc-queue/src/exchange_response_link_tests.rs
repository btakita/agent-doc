//! Tests for [`super`] (`#exchangeresponseid`).

use super::*;
use agent_doc_document::subagent_completion as completion;

const AT: &str = "2026-11-24T16:46:45.000Z";
const OWNER: &str = "subagent:selfsteer3";

/// The operator's live row quoted another response's id; its tree's id must
/// still be the one its exchange entry carries.
const ITEM: &str = "`[#r-35be26482ddb]` was not printed into the agent:exchange [#q-086ae2160fa7]";

const HANDBACK: &str = "**Summary**\n\nEvery response tree now has an exchange entry.\n\n**What changed**\n\nBranch `respid`, commit `abc1234`.\n\n**Verification**\n\n`cargo test -p agent-doc-queue` passed.\n\n**Action items**\n\nnone\n";

fn doc(queue: &str, exchange: &str) -> String {
    format!(
        "---\nqueue_active: true\n---\n\n<!-- agent:exchange -->\n### Re: earlier — opus\n\nEarlier answer.\n{exchange}<!-- /agent:exchange -->\n\n<!-- agent:queue -->\n{queue}<!-- /agent:queue -->\n"
    )
}

fn cell() -> String {
    completion::cell_with_response(OWNER, ITEM, "attemptkey", HANDBACK).unwrap()
}

/// The live document before and after the strike `report-complete` closes.
fn reported() -> (String, String) {
    let exchange = format!("\n{}\n", cell());
    let live = doc(&format!("- ☑️ [#{OWNER}#1] {ITEM}\n"), &exchange);
    let struck = doc(&format!("- ~~{ITEM}~~\n"), &exchange);
    (live, struck)
}

fn tree_id(document: &str) -> String {
    let queue = &document[document.find("<!-- agent:queue -->").unwrap()..];
    let at = queue.find("**Response [#r-").expect("a response tree") + "**Response ".len();
    let end = queue[at..].find(']').unwrap() + at + 1;
    queue[at..end].to_string()
}

fn exchange_body(document: &str) -> &str {
    let start = document.find("<!-- agent:exchange -->").unwrap();
    let end = document.find("<!-- /agent:exchange -->").unwrap();
    &document[start..end]
}

fn cell_heading(document: &str) -> &str {
    document
        .lines()
        .find(|line| line.starts_with(completion::HEADING_PREFIX))
        .expect("completion cell heading")
}

/// `report-complete` → strike: the hand-back's own completion cell carries
/// the tree's id on its heading, the tree's Details row names that id, and
/// the cell is still attested completion evidence.
#[test]
fn a_handback_tree_tags_its_completion_cell_with_its_id() {
    let (live, struck) = reported();
    let projected =
        crate::queue_consume::attach_stopped_row_responses(&live, &struck, "", AT).unwrap();
    let id = tree_id(&projected);
    let heading = cell_heading(&projected);
    assert!(
        heading.ends_with(&format!(" — agent-doc {id}")),
        "{heading}"
    );
    assert_eq!(
        agent_doc_markdown_ast::exchange_tree::response_id_from_heading(heading),
        Some(id.as_str()),
        "the response index reads the trailing id, not the quoted one"
    );
    assert_eq!(completion::completion_evidence(&projected).len(), 1);
    assert_eq!(completion::completion_evidence(&projected)[0].item, ITEM);
    assert!(
        projected.contains(&format!("hand-back in agent:exchange under {id}.")),
        "{projected}"
    );
    assert!(exchange_response_ids(exchange_body(&projected)).contains(&id));
    assert!(
        !exchange_response_ids(exchange_body(&projected)).contains("[#r-35be26482ddb]"),
        "a quoted id is not an entry"
    );
    // The completion cell is the entry: no extra record is written.
    assert!(!projected.contains(agent_doc_document::queue_response_record::HEADING_PREFIX));
    assert!(link_response_trees_to_exchange(Some(&struck), &projected).is_none());
    assert_eq!(
        crate::queue_consume::attach_stopped_row_responses(&live, &projected, "", AT).unwrap(),
        projected,
        "replaying the strike changes nothing"
    );
}

/// Retiring the cell keeps it as a hand-back record that still carries the
/// id, so the operator deleting the tree later cannot orphan the id.
#[test]
fn retiring_the_cell_keeps_its_id_on_the_handback_record() {
    let (live, struck) = reported();
    let projected =
        crate::queue_consume::attach_stopped_row_responses(&live, &struck, "", AT).unwrap();
    let id = tree_id(&projected);
    let (retired, keys) =
        crate::queue_consume::retire_consumed_completion_feedback(&projected, &[ITEM.to_string()]);
    assert_eq!(keys.len(), 1);
    assert!(completion::completion_evidence(&retired).is_empty());
    let record = retired
        .lines()
        .find(|line| line.starts_with(completion::HANDBACK_RECORD_HEADING_PREFIX))
        .expect("hand-back record");
    assert!(record.ends_with(&format!("” {id}")), "{record}");
    assert!(exchange_response_ids(exchange_body(&retired)).contains(&id));
    assert!(link_response_trees_to_exchange(None, &retired).is_none());
}

/// A tree whose cell is tagged is kept as a record even when the tree copied
/// every section (no Details row).
#[test]
fn a_tagged_cell_without_a_details_row_is_kept_as_a_record() {
    let (live, struck) = reported();
    let projected =
        crate::queue_consume::attach_stopped_row_responses(&live, &struck, "", AT).unwrap();
    let details = projected
        .lines()
        .find(|line| line.trim_start().starts_with("- **Details:**"))
        .unwrap()
        .to_string();
    let no_details = projected.replace(&format!("{details}\n"), "");
    let (retired, _) =
        crate::queue_consume::retire_consumed_completion_feedback(&no_details, &[ITEM.to_string()]);
    assert!(
        retired.contains(completion::HANDBACK_RECORD_HEADING_PREFIX),
        "{retired}"
    );
}

/// An older binary's document: the tree and the cell (or the record it was
/// retired to) are both present but unlinked. `queue sync` proves the link
/// from the attested hand-back and adds the id to the entry and the Details
/// row.
#[test]
fn queue_sync_repairs_an_unlinked_handback_entry() {
    let (live, struck) = reported();
    let projected =
        crate::queue_consume::attach_stopped_row_responses(&live, &struck, "", AT).unwrap();
    let id = tree_id(&projected);
    let legacy = projected
        .replace(&format!(" under {id}."), ".")
        .replace(&format!(" — agent-doc {id}"), " — agent-doc");
    assert!(!exchange_response_ids(exchange_body(&legacy)).contains(&id));

    let repaired = link_response_trees_to_exchange(None, &legacy).expect("linked");
    assert_eq!(repaired.annotated, vec![id.clone()]);
    assert_eq!(repaired.details, 1);
    assert!(repaired.recorded.is_empty());
    assert_eq!(repaired.document, projected);

    let (legacy_record, _) =
        crate::queue_consume::retire_consumed_completion_feedback(&legacy, &[ITEM.to_string()]);
    assert!(legacy_record.contains(completion::HANDBACK_RECORD_HEADING_PREFIX));
    let repaired = link_response_trees_to_exchange(None, &legacy_record).expect("linked");
    assert_eq!(repaired.annotated, vec![id.clone()]);
    assert!(exchange_response_ids(exchange_body(&repaired.document)).contains(&id));
    assert!(repaired.document.contains(&format!(" under {id}.")));
    assert!(link_response_trees_to_exchange(None, &repaired.document).is_none());
}

/// A hand-back that does not recompute to the tree's id is not its entry.
#[test]
fn an_unprovable_handback_is_left_alone() {
    let (live, struck) = reported();
    let projected =
        crate::queue_consume::attach_stopped_row_responses(&live, &struck, "", AT).unwrap();
    let id = tree_id(&projected);
    let other = "[#r-0123456789ab]";
    let forged = projected
        .replace(&format!(" under {id}."), ".")
        .replace(&format!(" — agent-doc {id}"), " — agent-doc")
        .replace(&id, other);
    assert!(link_response_trees_to_exchange(None, &forged).is_none());
}

/// An exact `--ack-text` strike's summary tree has no exchange response of
/// its own, so the strike writes an attested queue-response record carrying
/// its id. A repair pass never invents one for an older tree.
#[test]
fn an_ack_summary_tree_gets_a_queue_response_record() {
    let before = doc("- #justdoit\n", "");
    let after = doc("- ~~#justdoit~~\n", "");
    let attached = crate::queue_consume::attach_ack_summary_response(
        &before,
        &after,
        Some("Applied the three title edits."),
        AT,
    )
    .unwrap()
    .document;
    let id = tree_id(&attached);
    let exchange = exchange_body(&attached);
    assert!(exchange_response_ids(exchange).contains(&id), "{attached}");
    let heading = format!("### Re: Queue response: “#justdoit” — agent-doc {id}");
    assert!(exchange.contains(&format!("\n\n{heading}\n")), "{attached}");
    assert!(exchange.contains("> - **Summary:** Applied the three title edits.\n"));
    assert!(!exchange.contains("**Guidance:**"), "{exchange}");
    assert_eq!(
        agent_doc_document::queue_response_record::record_heading_lines(&attached).len(),
        1
    );
    assert!(link_response_trees_to_exchange(Some(&after), &attached).is_none());

    let unrecorded = attached.replace(
        &exchange[exchange.find(&format!("\n\n{heading}")).unwrap()..],
        "\n",
    );
    assert!(link_response_trees_to_exchange(None, &unrecorded).is_none());
}

/// A failed row's Response is a response too.
#[test]
fn a_failed_row_gets_a_queue_response_record() {
    let before = doc("- #address\n", "");
    let after = doc("- ~~#address~~\n", "");
    let failed = crate::queue_consume::mark_failed_rows(
        &before,
        &after,
        "The pull request was closed upstream.",
        &["re-fetched the PR".to_string()],
        AT,
    )
    .unwrap();
    let id = tree_id(&failed);
    assert!(
        exchange_response_ids(exchange_body(&failed)).contains(&id),
        "{failed}"
    );
    assert!(
        failed.contains("> - **State:** ❌ Failed (terminal)."),
        "{failed}"
    );
}

/// A tree minted from an exchange response (a coordinator `respond`) is
/// already in the input; it gains no record.
#[test]
fn a_tree_already_in_the_input_gains_no_record() {
    let tree = "- ~~do [#x]~~\n  - **Response [#r-0123456789ab]**\n    - **Summary:** Done.\n";
    let document = doc(tree, "");
    assert!(link_response_trees_to_exchange(Some(&document), &document).is_none());
}

/// Without an exchange component nothing is written.
#[test]
fn a_document_without_an_exchange_is_unchanged() {
    let before = "<!-- agent:queue -->\n- #justdoit\n<!-- /agent:queue -->\n";
    let after = "<!-- agent:queue -->\n- ~~#justdoit~~\n<!-- /agent:queue -->\n";
    let attached =
        crate::queue_consume::attach_ack_summary_response(before, after, Some("Done."), AT)
            .unwrap()
            .document;
    assert!(attached.contains("**Response [#r-"));
    assert!(!attached.contains("Queue response"));
}
