//! Regression tests over a trimmed live session document whose Waiting-on-you
//! tree had grown to 196 lines that the operator could no longer read
//! (`#waitingpacketowner`, `#waitingitemstatus`).

use std::collections::HashSet;

use crate::document_queue::{self, QueueEntry, QueueRowProjection};
use crate::queue_consume::{WAITING_ON_YOU_ITEM_MARKER, project_waiting_on_you_tree};
use crate::waiting_packet::{
    WaitingMember, classify_member, is_packet_row, is_waiting_item_head, packet_member_indexes,
};
use agent_doc_document::queue_projection::{QueueLifecycleMarker, lifecycle_marker};

const FIXTURE: &str = include_str!("../tests/fixtures/waiting_tree_session.md");

fn main_queue_body(document: &str) -> String {
    agent_doc_element::element::parse(document)
        .unwrap()
        .into_iter()
        .find(|component| {
            component.name == "queue"
                && component.attrs.get("id").map(String::as_str) == Some("main")
        })
        .expect("fixture has a main run")
        .content(document)
        .to_string()
}

fn main_entries(document: &str) -> Vec<QueueEntry> {
    document_queue::parse(&main_queue_body(document)).unwrap()
}

/// Every queue component rewritten through the lifecycle projection with no
/// claim ledger: every live row's target is `Pending`, exactly what preflight
/// builds for rows without a claim, receipt or completion cell.
fn project_pending_lifecycle(document: &str) -> String {
    let mut out = document.to_string();
    let components = agent_doc_element::element::parse(document).unwrap();
    for component in components
        .iter()
        .rev()
        .filter(|component| component.name == "queue")
    {
        let body = component.content(document);
        let entries = document_queue::parse(body).unwrap();
        let targets = entries
            .iter()
            .filter_map(|entry| match entry {
                QueueEntry::Prompt(prompt) => Some(QueueRowProjection {
                    text: prompt.text.clone(),
                    marker: QueueLifecycleMarker::Pending,
                    owner: None,
                    blockers: Vec::new(),
                }),
                _ => None,
            })
            .collect::<Vec<_>>();
        if let Some(marked) = document_queue::set_prompt_runtime_projections(&entries, &targets) {
            out.replace_range(
                component.open_end..component.close_start,
                &document_queue::render(&marked),
            );
        }
    }
    out
}

/// One maintenance cycle: closeout projection, stable-id admission, lifecycle
/// projection, and operator-action projection, in preflight order.
fn maintenance_cycle(document: &str) -> String {
    let mut current = project_waiting_on_you_tree(document, "")
        .unwrap()
        .unwrap_or_else(|| document.to_string());
    current = crate::prompt_identity::project(&current, "fixture").content;
    current = project_pending_lifecycle(&current);
    let open = agent_doc_element::todo::open_todo_ids(&current);
    if let Some(projected) =
        document_queue::project_all_prompt_operator_actions(&current, &[], &open).unwrap()
    {
        current = projected;
    }
    current
}

fn settled_fixture() -> String {
    maintenance_cycle(FIXTURE)
}

fn row_text(entry: &QueueEntry) -> Option<&str> {
    match entry {
        QueueEntry::Prompt(prompt) | QueueEntry::Completed(prompt) => Some(&prompt.text),
        _ => None,
    }
}

fn head_index(entries: &[QueueEntry], id: &str) -> usize {
    let needle = format!("[#{id}]");
    entries
        .iter()
        .position(|entry| is_waiting_item_head(entry) && row_text(entry).unwrap().contains(&needle))
        .unwrap_or_else(|| panic!("no waiting head for {id}"))
}

#[test]
fn every_item_of_the_fixture_is_a_waiting_item_after_closeout() {
    let settled = settled_fixture();
    let entries = main_entries(&settled);
    let heads = entries
        .iter()
        .filter(|entry| is_waiting_item_head(entry))
        .count();
    let live = entries
        .iter()
        .filter(|entry| matches!(entry, QueueEntry::Prompt(_)) && is_waiting_item_head(entry))
        .count();
    assert_eq!((heads, live), (24, 23), "{}", main_queue_body(&settled));
    // The legacy unmarked tree's items are adopted in place, never moved.
    for id in [
        "tdcraigunityhandoff",
        "tdsdkreleaseapp",
        "tdcontractsscenariospush",
    ] {
        head_index(&entries, id);
    }
}

#[test]
fn no_packet_row_is_a_queue_row() {
    let settled = settled_fixture();
    let entries = main_entries(&settled);
    for (index, entry) in entries.iter().enumerate() {
        if !is_packet_row(&entries, index) {
            continue;
        }
        let QueueEntry::Prompt(prompt) = entry else {
            continue;
        };
        assert_ne!(
            lifecycle_marker(&prompt.text),
            Some(QueueLifecycleMarker::Pending),
            "a packet row never carries a pending projection: {}",
            prompt.text
        );
        assert!(
            !crate::waiting_packet::is_binary_follow_up(&prompt.text)
                || agent_doc_document::queue_projection::owner_projection(&prompt.text).is_some()
                || crate::waiting_packet::subtree_end(&entries, index) > index + 1,
            "no bare synthesized follow-up row survives in a packet: {}",
            prompt.text
        );
    }
    let executable = document_queue::executable_prompt_indexes(&entries);
    for index in &executable {
        assert!(
            !is_packet_row(&entries, *index),
            "a packet row is never selectable: {:?}",
            row_text(&entries[*index])
        );
    }
    // Stable-id admission never reaches into a waiting tree.
    let admission = crate::prompt_identity::project(&settled, "fixture");
    let ranges = crate::waiting_packet::waiting_tree_byte_ranges(&settled);
    for assignment in admission.assignments {
        let at = settled.find(&assignment.source_text).unwrap();
        assert!(
            !ranges.iter().any(|range| range.contains(&at)),
            "a packet row was admitted a q- id: {}",
            assignment.source_text
        );
    }
}

#[test]
fn offered_choices_and_their_drifted_label_are_inert() {
    let settled = settled_fixture();
    let entries = main_entries(&settled);
    for entry in &entries {
        if let QueueEntry::Prompt(prompt) = entry {
            assert_ne!(
                prompt.text.trim(),
                "**Choices:**",
                "a Choices label is not work"
            );
            assert!(
                !prompt.text.starts_with("**Operator does it in the UI**"),
                "an undispatched offered choice is not work"
            );
        }
    }
}

#[test]
fn sibling_items_never_share_packet_rows() {
    let settled = settled_fixture();
    let entries = main_entries(&settled);
    let heads = (0..entries.len())
        .filter(|index| {
            matches!(entries[*index], QueueEntry::Prompt(_))
                && is_waiting_item_head(&entries[*index])
        })
        .collect::<Vec<_>>();
    let mut owners = std::collections::HashMap::new();
    for head in heads {
        for member in packet_member_indexes(&entries, head) {
            assert!(
                owners.insert(member, head).is_none(),
                "row {member} is owned by two waiting items"
            );
        }
    }
    // The top-level item that swallowed its siblings' rows by indentation
    // owns its own packet only; each nested sibling keeps its own.
    let oauth = head_index(&entries, "tdoauthcodestream");
    let handoff = head_index(&entries, "tdhandoffdocrelock");
    assert!(!packet_member_indexes(&entries, oauth).contains(&handoff));
    assert!(
        !crate::waiting_packet::has_operator_reply(&entries, oauth),
        "a nested sibling item is not an answer to its enclosing item"
    );
    assert!(
        !document_queue::executable_prompt_indexes(&entries).contains(&oauth),
        "an unanswered item is not selected by its nested siblings' answers"
    );
}

#[test]
fn operator_text_survives_byte_for_byte() {
    // Classify on the adopted original: adoption only appends hidden
    // sentinels, so operator rows are still byte-identical there.
    let adopted = project_waiting_on_you_tree(FIXTURE, "").unwrap().unwrap();
    let original = main_entries(&adopted);
    let mut operator_rows = Vec::new();
    for head in (0..original.len()).filter(|index| is_waiting_item_head(&original[*index])) {
        for member in packet_member_indexes(&original, head) {
            if let QueueEntry::Prompt(prompt) = &original[member]
                && classify_member(prompt) == WaitingMember::Operator
            {
                operator_rows.push(prompt.text.clone());
            }
        }
    }
    assert_eq!(operator_rows.len(), 13, "{operator_rows:#?}");
    assert!(
        operator_rows
            .iter()
            .any(|row| row == "tmstorm [#q-569d77146187]")
    );
    assert!(
        operator_rows
            .iter()
            .any(|row| row == "Go: HAVN-1178 should follow the #chat-policy. [#q-4ad6f250e52d]")
    );
    let settled = settled_fixture();
    let fixture_lines = FIXTURE.lines().collect::<Vec<_>>();
    for row in &operator_rows {
        // The row's exact bytes, minus at most a binary `📥 ` prefix, appear
        // in the original fixture and in the settled document alike.
        assert!(
            fixture_lines
                .iter()
                .any(|line| line.ends_with(row.as_str())),
            "fixture row: {row}"
        );
        assert!(
            settled
                .lines()
                .any(|line| line.trim_start() == format!("- {row}")),
            "operator row rewritten or lost: {row}"
        );
    }
}

#[test]
fn closeout_reaches_a_fixed_point_on_the_fixture() {
    let once = settled_fixture();
    let twice = maintenance_cycle(&once);
    assert_eq!(
        once,
        twice,
        "a second maintenance cycle must change nothing:\n{}",
        similar::TextDiff::from_lines(&once, &twice).unified_diff()
    );
    assert!(project_waiting_on_you_tree(&once, "").unwrap().is_none());
}

#[test]
fn a_claimed_waiting_item_keeps_its_attempt_projection() {
    let settled = settled_fixture();
    assert!(
        settled.contains(&format!(
            "  - 🙋☑️ [#subagent:c7685t43#1] [#5t43] Merge https://github.com/haiven-dev/haiven-infra/pull/318 so main drops the colliding docs-site IAM role tags. {WAITING_ON_YOU_ITEM_MARKER}"
        )),
        "{}",
        main_queue_body(&settled)
    );
    assert!(settled.contains("**Timestamp:** 2026-11-03T21:19-05:00"));
}

#[test]
fn answered_items_are_selected_once_and_reported_items_wait_for_reconciliation() {
    let settled = settled_fixture();
    let entries = main_entries(&settled);
    let executable = document_queue::executable_prompt_indexes(&entries)
        .into_iter()
        .collect::<HashSet<_>>();
    // Answered and idle: the item itself is the one selectable identity.
    assert!(executable.contains(&head_index(&entries, "tdprovauth")));
    // Choices and replies an older binary dispatched one worker each still
    // await reconciliation: the item is not dispatched a second time.
    assert!(!executable.contains(&head_index(&entries, "tdsdkreleaseapp")));
    // The item's own reported attempt keeps its claim live.
    assert!(executable.contains(&head_index(&entries, "tdhandoffdocrelock")));
}
