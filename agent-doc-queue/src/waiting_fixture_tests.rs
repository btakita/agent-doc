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
            "  - 🙋☑️ [#subagent:c7685t43#1] ⟦☑️ you: 0 · agent: 1 · done: 0⟧ [#5t43] Merge https://github.com/haiven-dev/haiven-infra/pull/318 so main drops the colliding docs-site IAM role tags. {WAITING_ON_YOU_ITEM_MARKER}"
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

/// The head line of the live waiting item `id` in the settled fixture.
fn head_line(settled: &str, id: &str) -> String {
    let needle = format!("[#{id}]");
    settled
        .lines()
        .find(|line| {
            line.contains(&needle)
                && line.contains(WAITING_ON_YOU_ITEM_MARKER)
                && !line.contains("~~")
        })
        .unwrap_or_else(|| panic!("no head line for {id}"))
        .to_string()
}

#[test]
fn every_live_head_carries_its_state_and_roll_up() {
    let settled = settled_fixture();
    let entries = main_entries(&settled);
    for entry in &entries {
        let QueueEntry::Prompt(prompt) = entry else {
            continue;
        };
        if !is_waiting_item_head(entry) {
            continue;
        }
        let work = agent_doc_element_queue::strip_attempt_projection_prefix(&prompt.text);
        let prefix = &prompt.text[..prompt.text.len() - work.len()];
        assert!(
            prefix.contains("you: ")
                && prefix.contains(" · agent: ")
                && prefix.contains(" · done: "),
            "a live waiting head without its state token: {}",
            prompt.text
        );
    }
    // The item the operator could not read: worker reports on every offered
    // choice, none integrated, one sibling decision still open.
    assert!(
        head_line(&settled, "tdsdkreleaseapp")
            .contains("⟦☑️ you: 1 · agent: 11 · done: 3⟧ [#tdsdkreleaseapp]"),
        "{}",
        head_line(&settled, "tdsdkreleaseapp")
    );
    // Answered and idle: waiting for the agent to pick the answer up.
    assert!(head_line(&settled, "tdprovauth").contains("⟦📥 you: 0 · agent: 1 · done: 0⟧"));
    // Unanswered: the operator owes the answer.
    assert!(
        head_line(&settled, "subagent-operator-request-craigbot-dm-1793755182")
            .contains("⟦you: 1 · agent: 0 · done: 0⟧")
    );
    // The item's own attempt reported; the agent must reconcile it.
    assert!(head_line(&settled, "tdhandoffdocrelock").contains("⟦☑️ you: 0 · agent: 1 · done: 0⟧"));
}

#[test]
fn the_head_lines_and_their_next_rows_alone_tell_each_items_status() {
    let settled = settled_fixture();
    let body = main_queue_body(&settled);
    let lines = body.lines().collect::<Vec<_>>();
    let mut items = 0usize;
    for (index, line) in lines.iter().enumerate() {
        if !line.contains(WAITING_ON_YOU_ITEM_MARKER) || line.contains("~~") {
            continue;
        }
        items += 1;
        let next = lines[index + 1];
        assert!(
            crate::waiting_status::is_next_row_at(&lines, index + 1),
            "the first child of a waiting head is its Next row:\n{line}\n{next}"
        );
        assert!(
            next.contains("**Next:** you — ")
                || next.contains("**Next:** agent — ")
                || next.contains("**Next:** none derivable"),
            "{next}"
        );
        assert!(next.contains("(evidence: "), "{next}");
        assert_eq!(
            (index + 1..lines.len())
                .take_while(|candidate| !lines[*candidate].contains(WAITING_ON_YOU_ITEM_MARKER))
                .filter(|candidate| crate::waiting_status::is_next_row_at(&lines, *candidate))
                .count(),
            1,
            "exactly one Next row per item: {line}"
        );
    }
    assert_eq!(items, 23);
    let release = lines
        .iter()
        .position(|line| {
            line.contains("[#tdsdkreleaseapp]") && line.contains(WAITING_ON_YOU_ITEM_MARKER)
        })
        .unwrap();
    assert_eq!(
        lines[release + 1],
        "    - **Next:** agent — integrate the reported results, then close the item or ask what remains (evidence: ☑️ `subagent:0f24a54c#1` and 10 more attempts) ▫️"
    );
}

/// `#barereplymarker`: the live fixture still carries the legacy hidden
/// `<!-- agent-doc:reply -->` comment on every generated row. One maintenance
/// cycle re-renders each of them as a bare trailing `▫️`, and every generated
/// row it re-renders is still recognized as binary-owned.
#[test]
fn legacy_reply_comments_migrate_to_the_bare_trailing_glyph() {
    let fixture = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/waiting_tree_session.md"
    ))
    .unwrap();
    assert!(
        main_queue_body(&fixture).contains(crate::queue_consume::LEGACY_AGENT_DOC_REPLY_MARKER),
        "the fixture exercises the legacy form"
    );
    let settled = settled_fixture();
    let body = main_queue_body(&settled);
    // The Waiting-on-you tree is what the refresh re-renders. (Items adopted
    // in place elsewhere in the queue are not re-rendered; their legacy rows
    // keep the comment and stay recognized through it.)
    let all_lines = body.lines().collect::<Vec<_>>();
    let root = all_lines
        .iter()
        .position(|line| line.contains(crate::queue_consume::WAITING_ON_YOU_ROOT_MARKER))
        .unwrap();
    let end = all_lines[root + 1..]
        .iter()
        .position(|line| !line.starts_with(' '))
        .map_or(all_lines.len(), |offset| root + 1 + offset);
    let tree = all_lines[root..end].join("\n");
    assert!(
        !tree.contains(crate::queue_consume::LEGACY_AGENT_DOC_REPLY_MARKER)
            && !tree.contains(crate::queue_consume::LEGACY_WAITING_METADATA_MARKER),
        "{tree}"
    );
    let lines = all_lines;
    let glyph = crate::queue_consume::AGENT_DOC_REPLY_GLYPH;
    let mut generated = 0usize;
    for (index, line) in lines.iter().enumerate() {
        if !line.ends_with(&format!(" {glyph}")) {
            continue;
        }
        generated += 1;
        assert!(
            agent_doc_markdown_ast::reply_row::is_agent_doc_reply_row_at(&lines, index),
            "a re-rendered generated row lost its ownership: {line}"
        );
    }
    assert!(generated > 23, "every item has at least its Next row: {generated}");
    // A fixed point: the next cycle changes nothing.
    assert_eq!(maintenance_cycle(&settled), settled);
}

// ---- #operatorrepliesnested -------------------------------------------------

const OPERATOR_REPLIES_FIXTURE: &str =
    include_str!("../tests/fixtures/waiting_tree_operator_replies.md");


/// The waiting items of the fixture whose packets hold an operator reply, with
/// that reply's exact text (any stale binary `📥 ` paint removed).
const OPERATOR_REPLIES: [(&str, &str); 5] = [
    (
        "rungamesessions-reopen",
        "Is this moot? agent-doc should do the reconciliation. [#q-4381636bab0e]",
    ),
    (
        "subagent-operator-request-vscode-coder-workspace",
        "moot or still needed? [#q-eeb402d8c55c]",
    ),
    (
        "subagent-operator-request-queue-ast-strings-persist",
        "commit + integrate + push",
    ),
    (
        "subagent-operator-request-parent-decision-response-disposition",
        "Why do you recommend Leave uncommitted?",
    ),
    (
        "subagent-operator-request-unique-child-vectors-persist",
        "commit + integrate + push",
    ),
];

fn operator_reply_index(entries: &[QueueEntry], id: &str, reply: &str) -> usize {
    let head = head_index(entries, id);
    packet_member_indexes(entries, head)
        .into_iter()
        .find(|member| row_text(&entries[*member]) == Some(reply))
        .unwrap_or_else(|| panic!("{id}: reply {reply:?} is not in its packet"))
}

/// `#operatorrepliesnested`: a waiting item's text quoted a truncated report
/// with an unbalanced backtick. The inline-code projection paired it with a
/// backtick two items later, so the next two waiting items and every row
/// beneath them read as inert text: their operator replies were invisible to
/// the packet scan, and the Waiting-on-you refresh regenerated both packets
/// from the Todo without them.
#[test]
fn an_unbalanced_backtick_in_one_item_never_hides_its_siblings() {
    let entries = main_entries(OPERATOR_REPLIES_FIXTURE);
    for id in [
        "subagent-operator-request-parent-decision-response-disposition",
        "subagent-operator-request-unique-child-vectors-persist",
    ] {
        let head = head_index(&entries, id);
        assert!(matches!(entries[head], QueueEntry::Prompt(_)), "{id}");
    }
    let reply = entries
        .iter()
        .position(|entry| row_text(entry) == Some("Why do you recommend Leave uncommitted?"))
        .expect("the operator reply is a list row, not inert text");
    assert!(is_packet_row(&entries, reply));
}

#[test]
fn operator_replies_nested_in_a_packet_are_decision_evidence_never_queue_rows() {
    let settled = maintenance_cycle(OPERATOR_REPLIES_FIXTURE);
    let entries = main_entries(&settled);
    let executable = document_queue::executable_prompt_indexes(&entries)
        .into_iter()
        .collect::<HashSet<_>>();
    let mut expected_executable = HashSet::new();
    for (id, reply) in OPERATOR_REPLIES {
        let head = head_index(&entries, id);
        let row = operator_reply_index(&entries, id, reply);
        let QueueEntry::Prompt(prompt) = &entries[row] else {
            panic!("{id}: reply {reply:?} is no longer live");
        };
        assert_eq!(classify_member(prompt), WaitingMember::Operator, "{reply}");
        assert_eq!(lifecycle_marker(&prompt.text), None, "{reply}");
        assert!(!executable.contains(&row), "a reply is never queue work: {reply}");
        // The reply selects its item, once, as decision evidence.
        assert!(crate::waiting_packet::has_operator_reply(&entries, head), "{id}");
        assert!(executable.contains(&head), "{id}");
        expected_executable.insert(head);
    }
    // A waiting item the rebuild retains because it holds operator text (for
    // example a duplicate of a Todo-backed item, `#waitingarchive`) is selected
    // by its reply exactly like the named items above.
    for (index, entry) in entries.iter().enumerate() {
        if crate::waiting_packet::is_waiting_item_head(entry)
            && crate::waiting_packet::has_operator_reply(&entries, index)
        {
            expected_executable.insert(index);
        }
    }
    // Nothing else in the tree is selectable: no offered choice, no generated
    // row, no struck row. Only the two ordinary top-level heads remain.
    for entry in &entries {
        if let QueueEntry::Prompt(prompt) = entry
            && prompt.indent == 0
            && !prompt.text.contains(crate::queue_consume::WAITING_ON_YOU_ROOT_MARKER)
        {
            expected_executable.insert(entries.iter().position(|e| e == entry).unwrap());
        }
    }
    assert_eq!(executable, expected_executable, "{}", main_queue_body(&settled));
    // The binary's stale `📥` paint is gone from every reply, and no reply was
    // admitted a fresh `q-` id.
    for line in main_queue_body(&settled).lines() {
        if !line.starts_with("    ") {
            continue;
        }
        assert!(!line.contains("📥"), "a packet row carries a pending projection: {line}");
    }
    let admission = crate::prompt_identity::project(&settled, "fixture");
    assert!(admission.assignments.is_empty(), "{:?}", admission.assignments);
    assert_eq!(maintenance_cycle(&settled), settled, "a fixed point");
}

#[test]
fn every_operator_row_survives_the_refresh_byte_for_byte() {
    let settled = maintenance_cycle(OPERATOR_REPLIES_FIXTURE);
    for (id, reply) in OPERATOR_REPLIES {
        let original = OPERATOR_REPLIES_FIXTURE
            .lines()
            .filter(|line| line.trim_start().trim_start_matches("- ").trim_start_matches("📥 ") == reply)
            .count();
        let kept = settled
            .lines()
            .filter(|line| line.trim_start() == format!("- {reply}"))
            .count();
        assert!(original >= 1 && kept == original, "{id}: {reply:?} {original} -> {kept}");
    }
    // Each packet keeps its own Response timestamp instead of a regenerated
    // placeholder.
    assert!(!settled.contains("1970-01-01"), "{}", main_queue_body(&settled));
}

/// `#operatorrepliesnested`: a closeout that struck a reply row, as the old
/// binary's consumption did for the `📥` rows it had painted, stays struck:
/// maintenance never revives a packet row as live queue work.
#[test]
fn maintenance_never_unstrikes_a_struck_packet_row() {
    let settled = maintenance_cycle(OPERATOR_REPLIES_FIXTURE);
    let mut struck = settled.clone();
    for (_, reply) in OPERATOR_REPLIES {
        let row = format!("- {reply}\n");
        let at = struck.find(&row).unwrap();
        struck.replace_range(at..at + row.len(), &format!("- ~~{reply}~~\n"));
    }
    let once = maintenance_cycle(&struck);
    for (_, reply) in OPERATOR_REPLIES {
        let row = format!("- ~~{reply}~~");
        assert!(
            once.lines().any(|line| line.trim_start() == row),
            "maintenance un-struck {reply:?}:\n{}",
            main_queue_body(&once)
        );
        assert!(!once.lines().any(|line| line.trim_start() == format!("- {reply}")));
    }
    for row in [
        "- ~~Resolve the open operator action to complete the parent item. [#rungamesessions-reopen] ⟦effect:2⟧~~",
        "- ~~Reconcile the reported work, then complete the parent item.~~",
    ] {
        assert!(once.lines().any(|line| line.trim_start() == row), "{row}");
    }
    let entries = main_entries(&once);
    for index in document_queue::executable_prompt_indexes(&entries) {
        assert!(!is_packet_row(&entries, index), "{:?}", row_text(&entries[index]));
    }
    assert_eq!(maintenance_cycle(&once), once, "a fixed point");
}

#[test]
fn offered_choices_in_a_waiting_packet_are_never_dispatched() {
    let settled = maintenance_cycle(OPERATOR_REPLIES_FIXTURE);
    let entries = main_entries(&settled);
    let executable = document_queue::executable_prompt_indexes(&entries);
    for choice in [
        "**Install and retry**:",
        "**Defer activation**:",
        "**Open/provide Coder workspace (Recommended)**:",
        "**Defer**:",
    ] {
        let line = main_queue_body(&settled)
            .lines()
            .find(|line| line.trim_start().starts_with(&format!("- {choice}")))
            .unwrap_or_else(|| panic!("{choice}"))
            .to_string();
        assert!(line.ends_with(crate::queue_consume::AGENT_DOC_REPLY_GLYPH), "{line}");
        assert!(
            !executable.iter().any(|index| row_text(&entries[*index])
                .is_some_and(|text| text.starts_with(choice))),
            "an offered choice is an operator option, never work: {choice}"
        );
    }
}

/// `#waitingarchive`: closing a waiting item's Todo moves its whole subtree,
/// operator answer included, into `agent:done` under one receipt. Nothing else
/// in the queue changes, and the next cycle is a fixed point.
#[test]
fn a_closed_items_subtree_moves_to_done_byte_for_byte() {
    use crate::waiting_archive::{WAITING_ARCHIVE_MARKER, unquote_archive_entry};

    let settled = settled_fixture();
    let body = main_queue_body(&settled);
    let start = body
        .split_inclusive('\n')
        .scan(0usize, |offset, line| {
            let at = *offset;
            *offset += line.len();
            Some((at, line))
        })
        .find(|(_, line)| {
            line.starts_with("  - ")
                && line.contains("[#sdkparitygate]")
                && line.contains(WAITING_ON_YOU_ITEM_MARKER)
        })
        .map(|(at, _)| at)
        .expect("the fixture's sdkparitygate waiting item");
    let rest = &body[start..];
    let len = rest
        .split_inclusive('\n')
        .enumerate()
        .take_while(|(index, line)| *index == 0 || !line.starts_with("  - ") && !line.starts_with("- "))
        .map(|(_, line)| line.len())
        .sum::<usize>();
    let segment = &rest[..len];
    assert!(segment.contains("verified [#q-79076461751e]"), "{segment}");

    let closed = settled.replacen("- [ ] [#sdkparitygate]", "- [x] [#sdkparitygate]", 1);
    let archived = maintenance_cycle(&closed);

    let after = main_queue_body(&archived);
    assert!(!after.contains("verified [#q-79076461751e]"), "{after}");
    assert_eq!(
        after,
        body.replacen(segment, "", 1),
        "only the closed item leaves the queue"
    );

    let done = agent_doc_element::element::parse(&archived)
        .unwrap()
        .into_iter()
        .find(|component| component.name == "done")
        .expect("the archive creates agent:done")
        .content(&archived)
        .to_string();
    let entry = &done[done.find("- ").expect("one receipt")..];
    let receipt = entry.lines().next().unwrap();
    assert!(receipt.contains("archived waiting item `#sdkparitygate` (its Todo closed)"), "{receipt}");
    assert!(receipt.ends_with(WAITING_ARCHIVE_MARKER), "{receipt}");
    assert_eq!(unquote_archive_entry(entry), segment, "every byte of the packet is kept");
    assert_eq!(done.matches(WAITING_ARCHIVE_MARKER).count(), 1);

    assert_eq!(
        maintenance_cycle(&archived),
        archived,
        "archiving reaches a fixed point"
    );
}

/// `#closedbygraft` (`#largewaitingtrees`): the Closed-by back-reference under
/// the `Operator does it in the UI` option re-copied the sibling options (the
/// Craig access choices) under `tdsdkreleaseapp`. Closeout drops the verbatim
/// copies and keeps the operator's `~~pick~~` with the option it answered.
#[test]
fn closed_by_back_references_no_longer_regraft_sibling_choices() {
    let settled = settled_fixture();
    let body = main_queue_body(&settled);
    let lines = body.lines().collect::<Vec<_>>();
    let option = lines
        .iter()
        .position(|line| line.trim_start().starts_with("- **Operator does it in the UI**"))
        .expect("the offered option row");
    let indent = lines[option].len() - lines[option].trim_start().len();
    let subtree = lines[option + 1..]
        .iter()
        .take_while(|line| line.len() - line.trim_start().len() > indent)
        .copied()
        .collect::<Vec<_>>();
    let joined = subtree.join("\n");
    for copied in [
        "**UI role**",
        "**Narrow app role**",
        "**Tester only**",
        "**Grant haiven-sdk only**",
        "**Operator does it in the UI**",
    ] {
        assert!(!joined.contains(copied), "{copied} re-copied:\n{joined}");
    }
    assert!(joined.contains("**Grant both**"), "{joined}");
    assert!(joined.contains("~~pick~~"), "the operator's answer stays:\n{joined}");
    assert!(
        joined.contains("**Request**: [#subagent-operator-request-craig-ui-read]"),
        "{joined}"
    );
    // The original offered options are still shown once, where they were offered.
    assert_eq!(body.matches("**Tester only**").count(), 1, "{body}");
}

/// A live document (2026-11-03 20:55) whose Waiting-on-you tree held an item
/// with no Todo id beside a duplicate of the same Todo
/// (`subagent-operator-request-queue-ast-strings-persist`).
const UNMATCHED_FIXTURE: &str = include_str!("../tests/fixtures/waiting_tree_unmatched_replies.md");

/// Operator replies of the unmatched-items fixture: list rows the binary did
/// not author. A leading `📥 ` is a stale binary projection that closeout
/// strips from packet rows (`#waitingpacketowner`); the operator's bytes are
/// the rest of the line.
const UNMATCHED_REPLIES: [&str; 6] = [
    "          - Is this moot? agent-doc should do the reconciliation. [#q-4381636bab0e]\n",
    "          - moot or still needed? [#q-eeb402d8c55c]\n",
    "          - commit + integration + push\n",
    "      - Why do you recommend Leave uncommitted?\n",
    "        - commit + integrate + push\n",
    "    - commit + integrate + push\n",
];

fn without_pending_projection(document: &str) -> String {
    document.replace("- 📥 ", "- ")
}

/// `#waitingarchive`: rebuilding the tree from the Todo list must never
/// delete operator text. An item with no Todo id (a duplicate of an open
/// Todo's item) used to vanish with its reply `commit + integration + push`;
/// it now stays in the tree, and every reply survives byte for byte.
#[test]
fn a_rebuild_keeps_every_operator_reply_of_unmatched_items() {
    let fixture = without_pending_projection(UNMATCHED_FIXTURE);
    for reply in UNMATCHED_REPLIES {
        assert!(fixture.contains(reply), "fixture reply: {reply}");
    }
    let once = project_waiting_on_you_tree(UNMATCHED_FIXTURE, "")
        .unwrap()
        .unwrap_or_else(|| UNMATCHED_FIXTURE.to_string());
    for reply in UNMATCHED_REPLIES {
        assert_eq!(
            without_pending_projection(&once).matches(reply).count(),
            fixture.matches(reply).count(),
            "operator reply lost or duplicated: {reply}\n{once}"
        );
    }
    let ast = "Answer the blocking subagent question for queue item ‘The update to agent-doc-bugs.md";
    let body = main_queue_body(&once);
    let unmatched = body
        .lines()
        .position(|line| line.starts_with("  - ") && line.contains(ast) && !line.contains("[#"))
        .unwrap_or_else(|| panic!("the unmatched item stays in the tree:\n{body}"));
    assert!(
        body.lines()
            .skip(unmatched)
            .take_while(|line| !line.starts_with("  - [#"))
            .any(|line| line == "          - commit + integration + push"),
        "the reply stays under its item:\n{body}"
    );
    assert!(!once.contains(crate::waiting_archive::WAITING_ARCHIVE_MARKER));
    let twice = maintenance_cycle(&maintenance_cycle(&once));
    for reply in UNMATCHED_REPLIES {
        assert!(
            without_pending_projection(&twice).contains(reply),
            "reply lost on a later cycle: {reply}"
        );
    }
    let thrice = maintenance_cycle(&twice);
    assert!(
        thrice == twice,
        "retaining unmatched items reaches a fixed point:\n{}",
        similar::TextDiff::from_lines(&twice, &thrice).unified_diff()
    );
}
