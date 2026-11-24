//! `#composefault` root: a binary write derived over the authority that already
//! carries the retained write before it is that write's successor, not a
//! concurrent branch of it.
//!
//! Measured on haiven-dev `tasks/sdk/sdk-dev.md`, 2026-11-24T21:45:24Z-21:45:44Z
//! (ops.log): the controller projected the retained report-complete chain
//! (`crdt_cp_write source=retained_transition_projection_effect
//! content_hash=6d7e28…`) while the editor had not acknowledged it. `queue
//! consume --ack-text` read that authority (`controller_crdt_current_text
//! text_hash=6d7e28…`), struck the row and wrote the result to the CP model
//! (`crdt_cp_write source=queue_consume_ack_text content_hash=b861…`). Its
//! retained successor, though, was a line-level 3-way merge of the older
//! target and the newer write over the chain's pre-write base
//! (`queue_consume_ack_text_deferred_write_composed requested_hash=b861…
//! composed_hash=27c2…`, `stale_queue_rows_reverted reverted=2`): the older
//! target's changes, already inside the newer write, were replayed a second
//! time against a different base. The controller then projected that
//! composition over the authority (`5406…`, 21:45:40), and the next consume
//! did the same again (`reverted=1`, `composed_hash=90fc…`). `#dupwrite`,
//! `#dupwrite2`, `#dupwrite3` and `#composefault` each repaired one shape of
//! that second replay.
//!
//! These drive report-complete then consume on the same row while the editor
//! never acknowledges and the operator types, with the `#composefault` rule off
//! (the outcome must be right without it) and on (it must rewrite nothing).
use super::*;
use agent_doc_document_realtime::concurrent_row_ops::probe;

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
const TYPED_MORE: &str = "- check the deploy log for the stg\n";
const TYPED: &str = "- check the deploy log for the stg rollout\n";

const WORK: &str = "I think we should use stateless JWTs. What is the reason against it?";
const HANDBACK: &str = concat!(
    "**Summary:** ranked the four credential models.\n",
    "**Action items:**\n",
    "- operator: choose one: (1) durable rows plus retention; (2) Model C, a stateless durable JWT with a denylist.\n",
    "- operator: hold #930 until you choose.\n",
);
const CELL: &str =
    "### Re: Subagent completed: `subagent:jwtadopt` — agent-doc\n\nRanked the four models.\n";
const RESPONSE_TREE: &str = concat!(
    "    - **Response [#r-ddc5f0eb1a59]**\n",
    "      - **Summary:** ranked the four credential models.\n",
    "      - **State:** Follow-up work remains.\n",
);

fn queue_lines(text: &str) -> Vec<&str> {
    let open = text.find("<!-- agent:queue").expect("queue open");
    let close = text.find("<!-- /agent:queue -->").expect("queue close");
    text[open..close].lines().skip(1).collect()
}

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

/// report-complete's successor over `base_rows`: `☑️` on the claim, the
/// origin's `[#q-…]` admitted, one follow-up row per action item after it, and
/// the completion cell.
fn report_complete(base_rows: &[&str]) -> (String, Vec<String>) {
    let claimed = format!("  - 🚧 [#subagent:jwtadopt#1] {WORK}\n");
    let reported = format!("  - ☑️ [#subagent:jwtadopt#1] {WORK}\n");
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
        &doc(CELL, &rows),
        "agent-doc:chainedwrite",
        WORK,
        HANDBACK,
    );
    assert_eq!(outcome.inserted.len(), 2, "{outcome:?}");
    (with_followups, outcome.inserted)
}

fn origin_id(followup: &str) -> String {
    followup
        .strip_prefix("[#")
        .and_then(|rest| rest.split_once("] follow-up:"))
        .map(|(id, _)| id.to_string())
        .expect("follow-up names its origin id")
}

/// `queue consume --ack-text` over `authority`: the reported row is struck,
/// keeping its id, and the hand-back's response tree attaches under it.
fn consume_over(authority: &str, origin_id: &str) -> String {
    let reported = format!("  - ☑️ [#subagent:jwtadopt#1] {WORK} [#{origin_id}]\n");
    assert_eq!(
        authority.matches(&reported).count(),
        1,
        "the authority carries the reported row once:\n{authority}"
    );
    authority.replacen(
        &reported,
        &format!("  - ~~{WORK} [#{origin_id}]~~\n{RESPONSE_TREE}"),
        1,
    )
}

/// The controller's retained projection of the pending intent over `editor`.
fn project(file: &std::path::Path, editor: &str) -> String {
    let pending = pending_document_write(file).expect("retained");
    agent_doc_document_realtime::write_policy::rebase_retained_target_over_editor_cut(
        pending.expected_content.as_deref().expect("retained base"),
        &pending.target_content,
        editor,
    )
    .unwrap()
}

fn ops_log(dir: &tempfile::TempDir) -> String {
    std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default()
}

/// The struck origin once with its id, its response tree directly under it,
/// both follow-ups after that subtree, the operator's row once, the cell once.
fn assert_struck_with_tree(label: &str, text: &str, id: &str, followups: &[String], typed: &str) {
    let origin = only_line(text, WORK);
    let lines = queue_lines(text);
    assert_eq!(
        lines[origin].trim(),
        format!("- ~~{WORK} [#{id}]~~"),
        "{label}: the origin is struck once and keeps its id:\n{text}"
    );
    assert!(
        lines[origin + 1].contains("**Response [#r-ddc5f0eb1a59]**"),
        "{label}: the response tree attaches directly to its origin:\n{text}"
    );
    let response_end = origin + 1 + RESPONSE_TREE.lines().count();
    for (offset, row) in followups.iter().enumerate() {
        assert_eq!(
            only_line(text, row),
            response_end + offset,
            "{label}: follow-up {offset} follows its origin's subtree:\n{text}"
        );
    }
    let typed_lines: Vec<&&str> = lines
        .iter()
        .filter(|line| line.contains("check the deploy log"))
        .collect();
    assert_eq!(
        typed_lines,
        vec![&typed.trim_end()],
        "{label}: the operator's row exists once, as typed:\n{text}"
    );
    assert_eq!(
        text.matches("### Re: Subagent completed: `subagent:jwtadopt`")
            .count(),
        1,
        "{label}: one completion cell:\n{text}"
    );
}

/// report-complete is retained; the controller projects it over the editor's
/// cut (the operator is typing) and the editor never acknowledges. consume
/// reads that authority and strikes the row. The retained successor is the
/// consume write itself; its projection over the editor's next cut is one
/// struck row with its tree, then the follow-ups, then the operator's row.
fn report_complete_then_consume_over_the_projected_authority() -> (String, String, String) {
    let claimed = format!("  - 🚧 [#subagent:jwtadopt#1] {WORK}\n");
    let base_rows = [PR, NOTE, claimed.as_str(), OTHER];
    let base = doc("", &base_rows);
    let (reported, followups) = report_complete(&base_rows);
    let (dir, file) = temp_doc(&base);
    ensure_deferred_document_write_intent(
        &file,
        &base,
        &reported,
        "queue_subagent_completion_feedback",
        DocumentWriteDeferredReason::EditorProjectionPending,
    )
    .unwrap();

    // The controller's projection over the operator's cut is the authority.
    let editor = doc("", &[PR, NOTE, claimed.as_str(), OTHER, TYPED_PARTIAL]);
    let authority = project(&file, &editor);
    // The projection admits the id onto the one row; no stale `🚧` copy.
    only_line(&authority, WORK);
    let consumed = consume_over(&authority, &origin_id(&followups[0]));
    ensure_deferred_document_write_intent(
        &file,
        &authority,
        &consumed,
        "queue_consume_ack_text",
        DocumentWriteDeferredReason::EditorProjectionPending,
    )
    .unwrap();
    let pending = pending_document_write(&file).expect("successor retained");
    assert_eq!(
        pending.target_content, consumed,
        "the successor derived over the authority carrying the retained write is that write's successor, not a composition"
    );

    // The editor still never acknowledged; the operator kept typing.
    let next_editor = doc("", &[PR, NOTE, claimed.as_str(), OTHER, TYPED]);
    let projected = project(&file, &next_editor);
    assert_struck_with_tree(
        "projected",
        &projected,
        &origin_id(&followups[0]),
        &followups,
        TYPED,
    );
    (pending.target_content, projected, ops_log(&dir))
}

#[test]
fn consume_over_the_projected_report_complete_is_its_successor_without_the_composefault_rule() {
    probe::with_disabled(report_complete_then_consume_over_the_projected_authority);
}

#[test]
fn consume_over_the_projected_report_complete_leaves_the_composefault_rule_nothing_to_rewrite() {
    let ((_, _, log), rewrites) =
        probe::count_rewrites(report_complete_then_consume_over_the_projected_authority);
    assert_eq!(rewrites, 0, "the #composefault rule rewrote a row:\n{log}");
    assert!(
        !log.contains("queue_consume_ack_text_deferred_write_composed"),
        "consume composed a second copy of the retained write:\n{log}"
    );
    assert!(
        log.contains("queue_consume_ack_text_deferred_write_superseded_by_derived_cut"),
        "consume superseded the retained write it was derived over:\n{log}"
    );
}

/// The second measured hop (21:45:43): the controller projects the consume
/// successor over the operator's next cut, and the next consume (a follow-up
/// row's strike) derives over that authority. Still one successor per write.
fn three_writes_chain_over_the_projected_authority() {
    let claimed = format!("  - 🚧 [#subagent:jwtadopt#1] {WORK}\n");
    let base_rows = [PR, NOTE, claimed.as_str(), OTHER];
    let base = doc("", &base_rows);
    let (reported, followups) = report_complete(&base_rows);
    let (_dir, file) = temp_doc(&base);
    ensure_deferred_document_write_intent(
        &file,
        &base,
        &reported,
        "queue_subagent_completion_feedback",
        DocumentWriteDeferredReason::EditorProjectionPending,
    )
    .unwrap();
    let editor = doc("", &[PR, NOTE, claimed.as_str(), OTHER, TYPED_PARTIAL]);
    let authority = project(&file, &editor);
    let consumed = consume_over(&authority, &origin_id(&followups[0]));
    ensure_deferred_document_write_intent(
        &file,
        &authority,
        &consumed,
        "queue_consume_ack_text",
        DocumentWriteDeferredReason::EditorProjectionPending,
    )
    .unwrap();

    let editor = doc("", &[PR, NOTE, claimed.as_str(), OTHER, TYPED_MORE]);
    let authority = project(&file, &editor);
    let id = origin_id(&followups[0]);
    assert_struck_with_tree("second authority", &authority, &id, &followups, TYPED_MORE);
    let first = format!("  - {}\n", followups[0]);
    assert_eq!(authority.matches(&first).count(), 1, "{authority}");
    let struck_first = format!("  - ~~{}~~\n", followups[0]);
    let second_consume = authority.replacen(&first, &struck_first, 1);
    ensure_deferred_document_write_intent(
        &file,
        &authority,
        &second_consume,
        "queue_consume_ack_text",
        DocumentWriteDeferredReason::EditorProjectionPending,
    )
    .unwrap();
    assert_eq!(
        pending_document_write(&file)
            .expect("retained")
            .target_content,
        second_consume
    );

    let editor = doc("", &[PR, NOTE, claimed.as_str(), OTHER, TYPED]);
    let projected = project(&file, &editor);
    let struck_followups = vec![format!("~~{}~~", followups[0]), followups[1].clone()];
    assert_struck_with_tree("projected", &projected, &id, &struck_followups, TYPED);
}

#[test]
fn three_writes_chain_over_the_projected_authority_without_the_composefault_rule() {
    probe::with_disabled(three_writes_chain_over_the_projected_authority);
}

#[test]
fn three_writes_chain_over_the_projected_authority_rewrite_nothing() {
    let ((), rewrites) = probe::count_rewrites(three_writes_chain_over_the_projected_authority);
    assert_eq!(rewrites, 0);
}
