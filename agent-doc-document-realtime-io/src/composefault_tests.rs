//! `#composefault`: two binary writes to ONE queue row, composed while the
//! editor never acknowledged the first.
//!
//! Measured on haiven-dev `tasks/sdk/sdk-dev.md`:
//!
//! - 2026-11-24T21:45:07Z-21:45:43Z (`stale_queue_rows_reverted` reverted=1/2/1
//!   at 21:45:07, 21:45:32, 21:45:43): `queue report-complete` for
//!   `subagent:jwtadopt#1` admitted the origin row's `[#q-b55e5bde5b7a]` and
//!   inserted its two follow-up rows after it, as a retained intent the editor
//!   (replaced replica, operator typing) never acknowledged. The strike derived
//!   over the editor's cut, without either change, struck the id-less row and
//!   attached the hand-back's response tree under it. Composing the two kept
//!   the row twice: `☑️ … [#q-b55e5bde5b7a]` with the follow-ups after it, then
//!   `~~…~~` with the response tree (c5815096).
//! - 2026-11-23 (38373132, 20:51:55Z): `subagent:w1`'s response tree
//!   `[#r-1c7433c63076]` landed under the last follow-up row instead of its
//!   origin `[#q-7f1e992b915b]`: the follow-up insert and the response attach
//!   both inserted right after the origin line, so the merge ordered one block
//!   before the other, and the response tree read as a child of the last
//!   follow-up (or the merge kept only one block).
//!
//! These race the two writes through the retained deferred-write composition
//! and the controller's retained projection over the editor's next cut, with
//! the operator typing in the same queue.
use super::*;

fn temp_doc(disk: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let file = dir.path().join("doc.md");
    std::fs::write(&file, disk).unwrap();
    agent_doc_crdt_relay_io::register_embedded_relay_route_for_file(&file).unwrap();
    (dir, file)
}

fn doc(exchange: &str, rows: &[&str]) -> String {
    format!(
        concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Keep going.\n",
            "{}",
            "<!-- /agent:exchange -->\n\n",
            "## Run <!-- agent:queue id=\"main\" subagents go -->\n",
            "{}",
            "<!-- /agent:queue -->\n",
        ),
        exchange,
        rows.concat()
    )
}

const PR: &str = "- ### https://github.com/acme/api/pull/930\n";
const NOTE: &str = "  - %% fix(auth): always issue durable credentials %%\n";
const OTHER: &str = "- ### https://github.com/acme/api/pull/933\n";
const TYPED_PARTIAL: &str = "- check the deploy log for\n";
const TYPED: &str = "- check the deploy log for the stg rollout\n";

fn queue_body(text: &str) -> &str {
    let open = text.find("<!-- agent:queue").expect("queue open");
    let close = text.find("<!-- /agent:queue -->").expect("queue close");
    &text[open..close]
}

/// The lines of `text`'s queue, without trailing newlines.
fn queue_lines(text: &str) -> Vec<&str> {
    queue_body(text).lines().skip(1).collect()
}

/// Index of the one queue line containing `needle`; fails on zero or several.
fn only_line(text: &str, needle: &str) -> usize {
    let hits: Vec<usize> = queue_lines(text)
        .iter()
        .enumerate()
        .filter(|(_, line)| line.contains(needle))
        .map(|(index, _)| index)
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "exactly one queue line must hold {needle:?}:\n{text}"
    );
    hits[0]
}

/// Compose an older retained intent (`older` over `base`) with a newer write
/// (`newer` over `cut`), then project the composed target over the editor's
/// `later_cut`. Returns both texts.
fn compose_and_project(
    base: &str,
    older: &str,
    older_source: &str,
    cut: &str,
    newer: &str,
    newer_source: &str,
    later_cut: &str,
) -> (String, String) {
    let (_dir, file) = temp_doc(base);
    ensure_deferred_document_write_intent(
        &file,
        base,
        older,
        older_source,
        DocumentWriteDeferredReason::EditorProjectionPending,
    )
    .unwrap();
    ensure_deferred_document_write_intent(
        &file,
        cut,
        newer,
        newer_source,
        DocumentWriteDeferredReason::EditorProjectionPending,
    )
    .unwrap();
    let pending = pending_document_write(&file).expect("composed chain retained");
    let projected =
        agent_doc_document_realtime::write_policy::rebase_retained_target_over_editor_cut(
            pending
                .expected_content
                .as_deref()
                .expect("composed intent keeps its base"),
            &pending.target_content,
            later_cut,
        )
        .unwrap();
    (pending.target_content, projected)
}

const JWT_WORK: &str = "I think we should use stateless JWTs. What is the reason against it?";
const JWT_HANDBACK: &str = concat!(
    "**Summary:** ranked the four credential models.\n",
    "**Action items:**\n",
    "- operator: choose one: (1) durable rows plus retention; (2) Model C, a stateless durable JWT with a denylist.\n",
    "- operator: hold #930 until you choose.\n",
);
const JWT_CELL: &str =
    "### Re: Subagent completed: `subagent:jwtadopt` — agent-doc\n\nRanked the four models.\n";
const JWT_RESPONSE_TREE: &str = concat!(
    "    - **Response [#r-ddc5f0eb1a59]**\n",
    "      - **Summary:** ranked the four credential models.\n",
    "      - **State:** Follow-up work remains.\n",
);

/// The report-complete successor: `☑️` on the claim, the origin's `[#q-…]`
/// admitted and one follow-up row per action item after it
/// (`plan_handback_followups_in`, the placement report-complete uses), and
/// the completion cell.
fn jwt_report_complete(base_rows: &[&str]) -> (String, Vec<String>) {
    let claimed = format!("  - 🚧 [#subagent:jwtadopt#1] {JWT_WORK}\n");
    let reported = format!("  - ☑️ [#subagent:jwtadopt#1] {JWT_WORK}\n");
    let rows: Vec<String> = base_rows
        .iter()
        .map(|row| {
            if *row == claimed {
                reported.clone()
            } else {
                row.to_string()
            }
        })
        .collect();
    let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
    let (with_followups, outcome) = agent_doc_queue::handback_followups::plan_handback_followups_in(
        &doc(JWT_CELL, &rows),
        "agent-doc:composefault",
        JWT_WORK,
        JWT_HANDBACK,
    );
    assert_eq!(outcome.inserted.len(), 2, "{outcome:?}");
    (with_followups, outcome.inserted)
}

/// The origin id a follow-up row names (`[#q-…] follow-up: …`).
fn followup_origin(row: &str) -> String {
    row.strip_prefix("[#")
        .and_then(|rest| rest.split_once("] follow-up:"))
        .map(|(id, _)| id.to_string())
        .expect("follow-up names its origin id")
}

/// `#composefault` (jwtadopt): report-complete's id admission + follow-up
/// insert races the strike + response attach of the same row. The row exists
/// once, struck with its id; the response tree sits directly under it; both
/// follow-ups follow its subtree; the operator's typed row survives.
#[test]
fn report_complete_followups_racing_the_strike_keep_one_row_and_attach_the_response_to_it() {
    let claimed = format!("  - 🚧 [#subagent:jwtadopt#1] {JWT_WORK}\n");
    let base_rows = [PR, NOTE, claimed.as_str(), OTHER];
    let base = doc("", &base_rows);
    let (reported, followups) = jwt_report_complete(&base_rows);
    let origin_id = followup_origin(&followups[0]);
    // The strike derived over the editor's cut: the operator has started a
    // row below; neither the id nor the follow-ups reached the editor.
    let cut = doc("", &[PR, NOTE, claimed.as_str(), OTHER, TYPED_PARTIAL]);
    let struck = format!("  - ~~{JWT_WORK}~~\n");
    let strike = doc(
        JWT_CELL,
        &[
            PR,
            NOTE,
            struck.as_str(),
            JWT_RESPONSE_TREE,
            OTHER,
            TYPED_PARTIAL,
        ],
    );
    let later_cut = doc("", &[PR, NOTE, claimed.as_str(), OTHER, TYPED]);
    let (composed, projected) = compose_and_project(
        &base,
        &reported,
        "queue_subagent_completion_feedback",
        &cut,
        &strike,
        "queue_consume_ack_text",
        &later_cut,
    );
    for (label, text) in [("composed", &composed), ("projected", &projected)] {
        let origin = only_line(text, JWT_WORK);
        let lines = queue_lines(text);
        assert_eq!(
            lines[origin].trim(),
            format!("- ~~{JWT_WORK} [#{origin_id}]~~"),
            "{label}: the origin is struck once and keeps its id:\n{text}"
        );
        assert!(
            lines[origin + 1].contains("**Response [#r-ddc5f0eb1a59]**"),
            "{label}: the response tree attaches directly to its origin:\n{text}"
        );
        let response_end = origin + 1 + JWT_RESPONSE_TREE.lines().count();
        for (offset, row) in followups.iter().enumerate() {
            let at = only_line(text, row);
            assert_eq!(
                at,
                response_end + offset,
                "{label}: follow-up {offset} follows its origin's subtree:\n{text}"
            );
        }
        let typed = if label == "composed" {
            TYPED_PARTIAL
        } else {
            TYPED
        };
        only_line(text, typed.trim_start_matches("- ").trim_end());
    }
}

const REPIN_ORIGIN: &str = "  - ~~[#q-7f1e992b915b] follow-up: agent: re-pin haiven-sdk #392 and #393 to contracts `1961c9b`. [#q-bd5a2af5f518]~~\n";
const REPIN_RESPONSE_TREE: &str = concat!(
    "    - **Response [#r-1c7433c63076]**\n",
    "      - **Summary:** re-pinned #392; #393 waits on golden_path.\n",
    "      - **State:** Follow-up work remains.\n",
);
const REPIN_CELL: &str =
    "### Re: Subagent completed: `subagent:w1` — agent-doc\n\nRe-pinned #392.\n";

/// `#composefault` (w1): the origin was struck earlier without a response.
/// report-complete's follow-up insert races the response attach under the
/// struck origin, both right after the origin line. The response tree stays
/// under its origin, and both follow-ups survive after it.
#[test]
fn followup_insert_racing_a_response_attach_keeps_the_tree_under_its_origin() {
    let f1 = "  - [#q-bd5a2af5f518] follow-up: agent: re-pin #393 once golden_path passes [#q-aa11bb22cc33]\n";
    let f2 = "  - [#q-bd5a2af5f518] follow-up: agent: on #399, update scripts/golden_path.py at re-pin [#q-dd44ee55ff66]\n";
    let base = doc("", &[PR, NOTE, REPIN_ORIGIN, OTHER]);
    let followups = doc(REPIN_CELL, &[PR, NOTE, REPIN_ORIGIN, f1, f2, OTHER]);
    let cut = doc("", &[PR, NOTE, REPIN_ORIGIN, OTHER, TYPED_PARTIAL]);
    let attach = doc(
        REPIN_CELL,
        &[
            PR,
            NOTE,
            REPIN_ORIGIN,
            REPIN_RESPONSE_TREE,
            OTHER,
            TYPED_PARTIAL,
        ],
    );
    let later_cut = doc("", &[PR, NOTE, REPIN_ORIGIN, OTHER, TYPED]);
    let (composed, projected) = compose_and_project(
        &base,
        &followups,
        "queue_subagent_completion_feedback",
        &cut,
        &attach,
        "queue_consume_ack_id",
        &later_cut,
    );
    for (label, text) in [("composed", &composed), ("projected", &projected)] {
        let origin = only_line(text, "re-pin haiven-sdk #392 and #393");
        let lines = queue_lines(text);
        assert!(
            lines[origin + 1].contains("**Response [#r-1c7433c63076]**"),
            "{label}: the response tree attaches directly to its origin:\n{text}"
        );
        let response_end = origin + 1 + REPIN_RESPONSE_TREE.lines().count();
        assert_eq!(
            only_line(text, "[#q-aa11bb22cc33]"),
            response_end,
            "{label}:\n{text}"
        );
        assert_eq!(
            only_line(text, "[#q-dd44ee55ff66]"),
            response_end + 1,
            "{label}:\n{text}"
        );
    }
}

/// The same race the other way round: the response attach is the retained
/// intent, and report-complete's follow-up insert is derived over a cut that
/// never received it.
#[test]
fn response_attach_retained_before_a_followup_insert_keeps_the_tree_under_its_origin() {
    let f1 = "  - [#q-bd5a2af5f518] follow-up: agent: re-pin #393 once golden_path passes [#q-aa11bb22cc33]\n";
    let base = doc("", &[PR, NOTE, REPIN_ORIGIN, OTHER]);
    let attach = doc(
        REPIN_CELL,
        &[PR, NOTE, REPIN_ORIGIN, REPIN_RESPONSE_TREE, OTHER],
    );
    let cut = doc("", &[PR, NOTE, REPIN_ORIGIN, OTHER, TYPED_PARTIAL]);
    let followups = doc(
        REPIN_CELL,
        &[PR, NOTE, REPIN_ORIGIN, f1, OTHER, TYPED_PARTIAL],
    );
    let later_cut = doc("", &[PR, NOTE, REPIN_ORIGIN, OTHER, TYPED]);
    let (composed, projected) = compose_and_project(
        &base,
        &attach,
        "queue_consume_ack_id",
        &cut,
        &followups,
        "queue_subagent_completion_feedback",
        &later_cut,
    );
    for (label, text) in [("composed", &composed), ("projected", &projected)] {
        let origin = only_line(text, "re-pin haiven-sdk #392 and #393");
        let lines = queue_lines(text);
        assert!(
            lines[origin + 1].contains("**Response [#r-1c7433c63076]**"),
            "{label}: the response tree attaches directly to its origin:\n{text}"
        );
        assert_eq!(
            only_line(text, "[#q-aa11bb22cc33]"),
            origin + 1 + REPIN_RESPONSE_TREE.lines().count(),
            "{label}:\n{text}"
        );
    }
}

/// The operator types a nested reply under the origin while report-complete's
/// follow-up insert is retained: the reply stays the origin's child, and the
/// follow-ups follow it.
#[test]
fn operator_reply_typed_under_the_origin_stays_its_child_beside_a_retained_followup_insert() {
    let claimed = format!("  - 🚧 [#subagent:jwtadopt#1] {JWT_WORK}\n");
    let base_rows = [PR, NOTE, claimed.as_str(), OTHER];
    let base = doc("", &base_rows);
    let (reported, followups) = jwt_report_complete(&base_rows);
    let reply_partial = "    - We are moving to stateless JWT.\n";
    let reply = "    - We are moving to stateless JWT. Use the logged out field.\n";
    let cut = doc("", &[PR, NOTE, claimed.as_str(), reply_partial, OTHER]);
    let (_dir, file) = temp_doc(&base);
    ensure_deferred_document_write_intent(
        &file,
        &base,
        &reported,
        "queue_subagent_completion_feedback",
        DocumentWriteDeferredReason::EditorProjectionPending,
    )
    .unwrap();
    let pending = pending_document_write(&file).expect("retained");
    let later_cut = doc("", &[PR, NOTE, claimed.as_str(), reply, OTHER]);
    for (label, editor) in [("cut", &cut), ("later_cut", &later_cut)] {
        let projected =
            agent_doc_document_realtime::write_policy::rebase_retained_target_over_editor_cut(
                pending.expected_content.as_deref().unwrap_or(&base),
                &pending.target_content,
                editor,
            )
            .unwrap();
        let origin = only_line(&projected, JWT_WORK);
        // Admitting the origin's id is not a retarget: the follow-ups name it.
        assert!(
            queue_lines(&projected)[origin]
                .ends_with(&format!("{JWT_WORK} [#{}]", followup_origin(&followups[0]))),
            "{label}: the origin keeps the id its follow-ups name:\n{projected}"
        );
        let typed = if label == "cut" { reply_partial } else { reply };
        let at = only_line(&projected, typed.trim().trim_start_matches("- "));
        assert_eq!(
            at,
            origin + 1,
            "{label}: the reply stays under its origin:\n{projected}"
        );
        for (offset, row) in followups.iter().enumerate() {
            assert_eq!(
                only_line(&projected, row),
                origin + 2 + offset,
                "{label}:\n{projected}"
            );
        }
    }
}
