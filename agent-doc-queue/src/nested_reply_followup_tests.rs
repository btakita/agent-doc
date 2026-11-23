//! `#nestedreplyfollowup` tests: the cases listed in
//! `specs/proposals/top-level-follow-up-for-nested-replies.md`, plus a live
//! claim, a second pass, and byte-for-byte operator text.

use super::*;

const DOC_ID: &str = "doc:nestedreply";

/// An operator reply under a struck row's response tree. The reply keeps the
/// operator's double space, a code span, and a non-ASCII character, so a
/// byte-for-byte move is observable.
const REPLY: &str = "Also check  the `gizmo` \u{2014} and its caf\u{e9} mode.";

fn session(rows: &str) -> String {
    format!(
        concat!(
            "---\nagent_doc_session: x\n---\n\n",
            "## Run <!-- agent:run id=\"main\" go -->\n",
            "{rows}",
            "<!-- /agent:run -->\n",
        ),
        rows = rows
    )
}

fn struck_tree(reply_line: &str) -> String {
    format!(
        concat!(
            "- ### ~~Fix the widget. [#q-aaaaaaaaaaaa]~~\n",
            "  - **Response [#r-56cf9a1ad146]**\n",
            "    - **Timestamp:** 2026-11-23T00:36:11.000Z\n",
            "    - **Summary:** I fixed the widget.\n",
            "    - **State:** Complete.\n",
            "    - **Guidance:** \u{1f5d1}\u{fe0f} Safe to delete this tree.\n",
            "{reply_line}",
        ),
        reply_line = reply_line
    )
}

fn document() -> String {
    session(&format!(
        "{}- ### do [#other]\n",
        struck_tree(&format!("    - {REPLY}\n"))
    ))
}

fn allow_all() -> (
    impl Fn(&NestedReplyRow) -> bool,
    impl Fn(&NestedReplyRow) -> bool,
) {
    (|_: &NestedReplyRow| false, |_: &NestedReplyRow| true)
}

fn run(document: &str) -> Option<(String, Vec<MovedNestedReply>)> {
    let (held, stable) = allow_all();
    move_nested_replies(
        document,
        DOC_ID,
        &NestedReplyGates {
            held: &held,
            stable: &stable,
        },
    )
}

fn queue_entries(document: &str) -> Vec<QueueEntry> {
    let component = agent_doc_element::element::parse(document)
        .unwrap()
        .into_iter()
        .find(|c| c.name == "queue")
        .unwrap();
    document_queue::parse(component.content(document)).unwrap()
}

#[test]
fn reply_under_a_struck_rows_response_becomes_a_top_level_row_with_a_link() {
    let before = document();
    let (after, moved) = run(&before).expect("the reply moves");
    let [record] = moved.as_slice() else {
        panic!("{moved:#?}");
    };
    let id = record.followup_id.as_str();
    assert!(id.starts_with("q-"), "{id}");
    assert_eq!(record.origin_id, "q-aaaaaaaaaaaa");
    assert_eq!(record.response_id, "r-56cf9a1ad146");
    assert_eq!(record.reply, REPLY);
    assert!(!record.linked_existing);
    assert_eq!(
        after,
        session(&format!(
            "{}- [#q-aaaaaaaaaaaa] follow-up: {REPLY} [#{id}]\n- ### do [#other]\n",
            struck_tree(&format!(
                "    - \u{21aa} [#{id}] <!-- agent-doc:followup-link -->\n"
            ))
        )),
        "one top-level row after the origin tree, a link in place"
    );
    // Byte-for-byte: the operator's text is in the new row unchanged, and
    // nowhere else.
    assert_eq!(after.matches(REPLY).count(), 1, "{after}");
    assert_eq!(
        record.followup_row,
        format!("[#q-aaaaaaaaaaaa] follow-up: {REPLY} [#{id}]")
    );
    // A second pass changes nothing.
    assert_eq!(run(&after), None, "re-sync is a no-op");
}

#[test]
fn a_cosmetic_pending_marker_is_not_moved_into_the_new_row() {
    let before = session(&struck_tree(&format!("    - \u{1f4e5} {REPLY}\n")));
    let (after, moved) = run(&before).expect("a 📥 reply still moves");
    let id = &moved[0].followup_id;
    assert!(
        after.contains(&format!(
            "\n- [#q-aaaaaaaaaaaa] follow-up: {REPLY} [#{id}]\n"
        )),
        "{after}"
    );
    assert!(!after.contains('\u{1f4e5}'), "{after}");
}

#[test]
fn a_live_claimed_nested_reply_is_never_moved() {
    let before = document();
    let entries = queue_entries(&before);
    let reply_index = entries
        .iter()
        .position(|entry| matches!(entry, QueueEntry::Prompt(p) if p.text == REPLY))
        .unwrap();
    let claimed = crate::queue_claim::claim_identity_at(&entries, reply_index).unwrap();
    // The gate the IO layer builds from the claim ledger: the reply's nest
    // identity is held.
    let held = |row: &NestedReplyRow| row.identity == claimed;
    let stable = |_: &NestedReplyRow| true;
    assert_eq!(
        move_nested_replies(
            &before,
            DOC_ID,
            &NestedReplyGates {
                held: &held,
                stable: &stable,
            }
        ),
        None
    );
    // A claim marker on the row itself also keeps it, whatever the gates say.
    for marker in [
        "\u{1f7e1}",
        "\u{1f6a7} [#subagent:gizmo#1]",
        "\u{2611}\u{fe0f}",
    ] {
        let claimed_row = session(&struck_tree(&format!("    - {marker} {REPLY}\n")));
        assert_eq!(run(&claimed_row), None, "{marker}");
    }
}

#[test]
fn a_reply_typed_this_cycle_waits_for_the_next_stable_baseline() {
    let before = document();
    let held = |_: &NestedReplyRow| false;
    // The stable baseline predates the reply.
    let old_baseline = session(&format!("{}- ### do [#other]\n", struck_tree("")));
    let in_baseline =
        |baseline: &str, row: &NestedReplyRow| baseline.lines().any(|line| line == row.line);
    let stable_old = |row: &NestedReplyRow| in_baseline(&old_baseline, row);
    assert_eq!(
        move_nested_replies(
            &before,
            DOC_ID,
            &NestedReplyGates {
                held: &held,
                stable: &stable_old,
            }
        ),
        None,
        "not moved while the line is newer than the baseline"
    );
    // The next baseline holds the line unchanged: now it moves.
    let stable_new = |row: &NestedReplyRow| in_baseline(&before, row);
    assert!(
        move_nested_replies(
            &before,
            DOC_ID,
            &NestedReplyGates {
                held: &held,
                stable: &stable_new,
            }
        )
        .is_some()
    );
}

#[test]
fn a_waiting_item_packet_answer_is_not_moved() {
    let item = "[#lzpypidevrevoke] Remove the old PyPI API token.";
    let waiting = session(&format!(
        concat!(
            "- ### Waiting on you \u{2014} answer by adding a child line under an item <!-- agent-doc:waiting-on-you -->\n",
            "  - \u{27e6}\u{1f4e5} you: 0 \u{b7} agent: 1 \u{b7} done: 0\u{27e7} ~~{item}~~ <!-- agent-doc:waiting-item -->\n",
            "    - **Response [#r-56cf9a1ad146]**\n",
            "      - **Summary:** Removed it.\n",
            "      - try again with the fresh login.\n",
        ),
        item = item
    ));
    assert_eq!(run(&waiting), None);
}

#[test]
fn a_reply_under_a_followups_response_chains_after_the_previous_followup() {
    let before = session(concat!(
        "- ### ~~Fix the widget. [#q-aaaaaaaaaaaa]~~\n",
        "  - **Response [#r-111111111111]**\n",
        "    - **Summary:** Fixed.\n",
        "- ### ~~[#q-aaaaaaaaaaaa] follow-up: check the gizmo. [#q-bbbbbbbbbbbb]~~\n",
        "  - **Response [#r-222222222222]**\n",
        "    - **Summary:** The gizmo is fine.\n",
        "    - What about the sprocket?\n",
        "- [#q-aaaaaaaaaaaa] follow-up: and the flange. [#q-cccccccccccc]\n",
        "- ### do [#other]\n",
    ));
    let (after, moved) = run(&before).expect("the reply moves");
    let id = &moved[0].followup_id;
    assert_eq!(
        moved[0].origin_id, "q-aaaaaaaaaaaa",
        "the chain names the first origin, so it stays flat"
    );
    assert_eq!(
        after,
        session(&format!(
            concat!(
                "- ### ~~Fix the widget. [#q-aaaaaaaaaaaa]~~\n",
                "  - **Response [#r-111111111111]**\n",
                "    - **Summary:** Fixed.\n",
                "- ### ~~[#q-aaaaaaaaaaaa] follow-up: check the gizmo. [#q-bbbbbbbbbbbb]~~\n",
                "  - **Response [#r-222222222222]**\n",
                "    - **Summary:** The gizmo is fine.\n",
                "    - \u{21aa} [#{id}] <!-- agent-doc:followup-link -->\n",
                "- [#q-aaaaaaaaaaaa] follow-up: and the flange. [#q-cccccccccccc]\n",
                "- [#q-aaaaaaaaaaaa] follow-up: What about the sprocket? [#{id}]\n",
                "- ### do [#other]\n",
            ),
            id = id
        ))
    );
    assert_eq!(run(&after), None);
}

#[test]
fn two_replies_in_one_tree_chain_in_order() {
    let before = session(&format!(
        "{}- ### do [#other]\n",
        struck_tree("    - First question?\n    - Second question?\n")
    ));
    let (after, moved) = run(&before).expect("both move");
    assert_eq!(moved.len(), 2, "{after}");
    let first = after.find("follow-up: First question?").expect("first row");
    let second = after
        .find("follow-up: Second question?")
        .expect("second row");
    let other = after.find("- ### do [#other]").unwrap();
    assert!(first < second && second < other, "{after}");
    assert_eq!(run(&after), None);
}

#[test]
fn dispatch_runs_the_new_row_once_and_never_the_link() {
    let (after, moved) = run(&document()).unwrap();
    let id = moved[0].followup_id.clone();
    let entries = queue_entries(&after);
    let executable: Vec<String> = document_queue::executable_prompt_indexes(&entries)
        .into_iter()
        .map(|index| match &entries[index] {
            QueueEntry::Prompt(prompt) => prompt.text.clone(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        executable,
        vec![
            format!("[#q-aaaaaaaaaaaa] follow-up: {REPLY} [#{id}]"),
            "do [#other]".to_string(),
        ]
    );
    assert!(
        entries.iter().any(|entry| matches!(
            entry,
            QueueEntry::Freeform(line) if line.contains(FOLLOWUP_LINK_MARKER)
        )),
        "the link row is inert: {entries:#?}"
    );
    // The new row is keyed like any top-level follow-up row `queue
    // follow-up` writes, never by the old nest vector.
    let row = entries
        .iter()
        .position(|entry| matches!(entry, QueueEntry::Prompt(p) if p.text.contains(REPLY)))
        .unwrap();
    let identity = crate::queue_claim::claim_identity_at(&entries, row).unwrap();
    assert_eq!(
        identity,
        crate::queue_claim::claim_identity(&format!(
            "[#q-aaaaaaaaaaaa] follow-up: {REPLY} [#{id}]"
        ))
    );
    assert_ne!(
        identity,
        crate::queue_claim::claim_identity(&moved[0].nest_vector)
    );
}

#[test]
fn an_equal_chained_followup_is_linked_not_duplicated() {
    let before = session(&format!(
        "{}- ~~[#q-aaaaaaaaaaaa] follow-up: {REPLY} [#q-dddddddddddd]~~\n",
        struck_tree(&format!("    - {REPLY}\n"))
    ));
    let (after, moved) = run(&before).expect("the reply is linked");
    assert!(moved[0].linked_existing);
    assert_eq!(moved[0].followup_id, "q-dddddddddddd");
    assert_eq!(
        after,
        session(&format!(
            "{}- ~~[#q-aaaaaaaaaaaa] follow-up: {REPLY} [#q-dddddddddddd]~~\n",
            struck_tree("    - \u{21aa} [#q-dddddddddddd] <!-- agent-doc:followup-link -->\n")
        ))
    );
}

#[test]
fn uncertain_shapes_stay_nested() {
    let cases = [
        // The row owning the response is live: the reply may be steering.
        session(concat!(
            "- ### Fix the widget. [#q-aaaaaaaaaaaa]\n",
            "  - **Response [#r-56cf9a1ad146]**\n",
            "    - **Summary:** Working on it.\n",
            "    - Also check the gizmo.\n",
        )),
        // A reply to another reply.
        session(&struck_tree(
            "    - Also check the gizmo.\n      - and the sprocket.\n",
        )),
        // A reply outside any response tree.
        session("- ### ~~Fix the widget. [#q-aaaaaaaaaaaa]~~\n  - Also check the gizmo.\n"),
        // An id-less struck origin cannot be named.
        session(concat!(
            "- ### ~~Fix the widget.~~\n",
            "  - **Response [#r-56cf9a1ad146]**\n",
            "    - **Summary:** Fixed.\n",
            "    - Also check the gizmo.\n",
        )),
        // Directives and presets keep their nested meaning.
        session(&struck_tree("    - #justdoit\n")),
        session(&struck_tree("    - do [#gizmo]\n")),
        session(&struck_tree("    - /agent-doc compact\n")),
        // A follow-up row's own owner label stays with operator-owned rows.
        session(&struck_tree("    - operator: decide on the gizmo.\n")),
    ];
    for case in cases {
        assert_eq!(run(&case), None, "{case}");
    }
}

#[test]
fn a_loop_queue_reply_stays_nested() {
    let looped = format!(
        concat!(
            "---\nagent_doc_session: x\n---\n\n",
            "<!-- agent:loop id=\"periodic\" -->\n",
            "{tree}",
            "<!-- /agent:loop -->\n",
        ),
        tree = struck_tree("    - Also check the gizmo.\n")
    );
    assert_eq!(run(&looped), None);
}

#[test]
fn link_rows_parse() {
    assert_eq!(
        followup_link_target(&followup_link_row_text("q-0123456789ab")),
        Some("q-0123456789ab")
    );
    assert_eq!(followup_link_target("\u{21aa} [#q-1] trailing"), None);
    assert_eq!(followup_link_target("plain text"), None);
}

#[test]
fn blank_lines_inside_and_after_the_tree_keep_the_tree_whole() {
    let before = session(concat!(
        "- ### ~~Fix the widget. [#q-aaaaaaaaaaaa]~~\n",
        "  - **Response [#r-56cf9a1ad146]**\n",
        "    - **Summary:** Fixed.\n",
        "    - Also check the gizmo.\n",
        "\n",
        "    - **Guidance:** keep.\n",
        "\n",
        "- ### do [#other]\n",
    ));
    let (after, moved) = run(&before).unwrap();
    let id = &moved[0].followup_id;
    assert_eq!(
        after,
        session(&format!(
            concat!(
                "- ### ~~Fix the widget. [#q-aaaaaaaaaaaa]~~\n",
                "  - **Response [#r-56cf9a1ad146]**\n",
                "    - **Summary:** Fixed.\n",
                "    - \u{21aa} [#{id}] <!-- agent-doc:followup-link -->\n",
                "\n",
                "    - **Guidance:** keep.\n",
                "- [#q-aaaaaaaaaaaa] follow-up: Also check the gizmo. [#{id}]\n",
                "\n",
                "- ### do [#other]\n",
            ),
            id = id
        ))
    );
}

#[test]
fn the_moved_rows_prompt_context_names_its_origin_and_response() {
    let (after, moved) = run(&document()).unwrap();
    let row = moved[0].followup_row.clone();
    let submitted = crate::queue_response::submitted_prompts_with_context_for_heads(
        &after,
        std::slice::from_ref(&row),
    );
    let [prompt] = submitted.as_slice() else {
        panic!("{submitted:#?}");
    };
    let (packet, body) = agent_doc_prompt_context::lineage::split(prompt);
    let packet = packet.expect("a context packet");
    let response = packet.response.expect("the response context is kept");
    assert_eq!(response.response_id.as_deref(), Some("r-56cf9a1ad146"));
    assert_eq!(response.summary, "I fixed the widget.");
    assert!(
        body.starts_with(&row),
        "the title names the origin id: {body}"
    );
    // A follow-up row written by `queue follow-up` (no link) is unchanged.
    let plain = session("- [#q-aaaaaaaaaaaa] follow-up: check the gizmo. [#q-eeeeeeeeeeee]\n");
    let entries = queue_entries(&plain);
    assert_eq!(
        linked_response_context(
            &entries,
            "[#q-aaaaaaaaaaaa] follow-up: check the gizmo. [#q-eeeeeeeeeeee]"
        ),
        None
    );
}

/// The tree as preflight and sync leave it once the reply is in a baseline:
/// the reply reopened its struck parent (`#struckparentkids`), got its own
/// `[#q-…]`, and a `📥`.
fn reopened_tree(replies: &str) -> String {
    format!(
        concat!(
            "- ### Fix the widget. [#q-aaaaaaaaaaaa]\n",
            "  - **Response [#r-56cf9a1ad146]**\n",
            "    - **Summary:** I fixed the widget.\n",
            "    - **State:** Complete.\n",
            "    - **Guidance:** \u{1f4cc} Keep: 1 open.\n",
            "{replies}",
        ),
        replies = replies
    )
}

#[test]
fn a_reply_that_reopened_its_parent_moves_with_its_own_id_and_restrikes_the_parent() {
    let before = session(&format!(
        "{}- ### do [#other]\n",
        reopened_tree(&format!("    - \u{1f4e5} {REPLY} [#q-db447a4add9c]\n"))
    ));
    let (after, moved) = run(&before).expect("the reply moves");
    assert_eq!(
        moved[0].followup_id, "q-db447a4add9c",
        "the reply keeps its id"
    );
    assert!(moved[0].owner_restruck);
    assert_eq!(
        after,
        session(&format!(
            concat!(
                "- ### ~~Fix the widget. [#q-aaaaaaaaaaaa]~~\n",
                "  - **Response [#r-56cf9a1ad146]**\n",
                "    - **Summary:** I fixed the widget.\n",
                "    - **State:** Complete.\n",
                "    - **Guidance:** \u{1f4cc} Keep: 1 open.\n",
                "    - \u{21aa} [#q-db447a4add9c] <!-- agent-doc:followup-link -->\n",
                "- [#q-aaaaaaaaaaaa] follow-up: {reply} [#q-db447a4add9c]\n",
                "- ### do [#other]\n",
            ),
            reply = REPLY
        ))
    );
    let entries = queue_entries(&after);
    assert!(
        matches!(&entries[0], QueueEntry::Completed(p) if p.text == "Fix the widget. [#q-aaaaaaaaaaaa]"),
        "the parent is closed again, not a new leaf: {entries:#?}"
    );
    assert_eq!(run(&after), None, "re-sync is a no-op");
}

#[test]
fn a_reopened_parent_is_restruck_only_when_its_last_reply_leaves() {
    let before = session(&reopened_tree(concat!(
        "    - First question? [#q-111111111111]\n",
        "    - Second question? [#q-222222222222]\n",
    )));
    let (held_first, stable) = (
        |row: &NestedReplyRow| row.reply == "Second question?",
        |_: &NestedReplyRow| true,
    );
    let (one, moved) = move_nested_replies(
        &before,
        DOC_ID,
        &NestedReplyGates {
            held: &held_first,
            stable: &stable,
        },
    )
    .expect("the unheld reply moves");
    assert_eq!(moved.len(), 1);
    assert!(!moved[0].owner_restruck, "a live reply is still under it");
    assert!(
        one.contains("- ### Fix the widget. [#q-aaaaaaaaaaaa]\n"),
        "{one}"
    );
    let (two, moved) = run(&one).expect("the second reply moves");
    assert!(moved[0].owner_restruck);
    assert!(
        two.contains("- ### ~~Fix the widget. [#q-aaaaaaaaaaaa]~~\n"),
        "{two}"
    );
}

#[test]
fn a_live_parent_that_is_not_a_finished_response_owner_keeps_its_replies() {
    let cases = [
        // Its response does not say the work finished.
        reopened_tree("    - Also check the gizmo?\n")
            .replace("**State:** Complete.", "**State:** Blocked on review."),
        // A worker attempt marks the parent.
        reopened_tree("    - Also check the gizmo?\n")
            .replace("- ### Fix", "- ### \u{1f6a7} [#subagent:widget#1] Fix"),
    ];
    for case in cases {
        let doc = session(&case);
        assert_eq!(run(&doc), None, "{doc}");
    }
}
