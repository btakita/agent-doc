//! `#dupdone`: the 2026-11-23 live shapes, private names replaced.

use super::*;

const LIVE_COPY: &str = "- ### 🙋 [from:chat key=report] do the operator's top-level open-work count request (each top-level row shows a synced count like (1 decision, 1 running)), and fix: a row that quotes a [operator-verify] token inside a code span is misread as an operator-verify row and refused for dispatch. #spec-test-commit-push [#q-aaaaaaaaaaaa]";

const TWIN_TEXT: &str = "[from:chat key=report2] do the operator's top-level open-work count request (each top-level row shows a synced count like (1 decision, 1 running)), and fix: a row that quotes the operator-verify bracket token inside a code span is misread as an operator-verify row and refused for dispatch. #spec-test-commit-push [#q-bbbbbbbbbbbb]";

fn never_in_flight(_: &str) -> bool {
    false
}

fn session(queue: &str, exchange: &str) -> String {
    format!(
        "---\nagent_doc_session: test\n---\n\n## Exchange <!-- agent:exchange patch=append -->\n{exchange}<!-- /agent:exchange -->\n\n## Run <!-- agent:run id=\"main\" priority go -->\n{queue}<!-- /agent:run -->\n"
    )
}

fn echo(heading: &str, prompt: &str, body: &str) -> String {
    format!("### {heading}\n\n> **Queue prompt:**\n>\n> {prompt}\n\n{body}\n\n")
}

/// The twin was dispatched, struck, and its follow-ups name the commit.
fn twin_rows() -> String {
    format!(
        concat!(
            "- ### ~~{twin}~~\n",
            "- 👀 [#q-bbbbbbbbbbbb] follow-up: agent: check that CI run 45094135922 on `9d07673718` finishes green. [#q-dddddddddddd]\n",
            "- ### ~~[#q-bbbbbbbbbbbb] follow-up: agent: run `make install` in the tool submodule and verify `tool --version`. [#q-cccccccccccc]~~\n",
            "  - **Response [#r-2500918c17d7]**\n",
            "    - **Summary:** Installed tool main 9d07673718 (build 0.35.586+de31b949045c6e45).\n",
            "    - **State:** Complete.\n",
        ),
        twin = TWIN_TEXT
    )
}

fn exchange_with_echo() -> String {
    echo(
        "Re: phase 2 · top-level counts · memory",
        TWIN_TEXT,
        "**Phase 2: landed** (`64ba7bf2fe`).\n\n**Top-level counts: landed** (`9d07673718`).",
    )
}

fn reconcile(document: &str) -> Option<String> {
    reconcile_finished_rows(document, Some(document), &never_in_flight).unwrap()
}

fn live_line(document: &str, needle: &str) -> Option<String> {
    document
        .lines()
        .find(|line| line.contains(needle))
        .map(str::to_string)
}

#[test]
fn a_requeued_duplicate_of_finished_work_is_struck_with_a_response() {
    let document = session(
        &format!("{LIVE_COPY}\n{}", twin_rows()),
        &exchange_with_echo(),
    );
    let resolved = reconcile(&document).expect("the live duplicate is resolved");
    let struck = live_line(&resolved, "key=report]").unwrap();
    assert!(
        struck.starts_with("- ### ~~[from:chat key=report] do the operator's")
            && struck.ends_with("— auto-struck: done by #q-bbbbbbbbbbbb (#dupdone)~~"),
        "{struck}"
    );
    assert!(!struck.contains('🙋'), "{struck}");
    let below = resolved
        .lines()
        .skip_while(|line| !line.contains("key=report]"))
        .skip(1)
        .take(3)
        .collect::<Vec<_>>();
    assert!(below[0].starts_with("  - **Response [#r-"), "{resolved}");
    assert!(
        below[1].starts_with("    - **Summary:** Duplicate of finished work: done by `[#q-bbbbbbbbbbbb]`/`9d07673718`"),
        "{resolved}"
    );
    assert_eq!(below[2], "    - **State:** Complete.");
    assert!(
        crate::struck_prompt_response::unanswered_struck_prompt_rows(&resolved).is_empty(),
        "{resolved}"
    );
    // Idempotent, and the twin and its follow-ups are untouched.
    assert_eq!(reconcile(&resolved), None);
    assert!(resolved.contains(&format!("- ### ~~{TWIN_TEXT}~~\n")));
    assert!(resolved.contains("- 👀 [#q-bbbbbbbbbbbb] follow-up: agent: check that CI run"));
    // It is no longer an operator loose end.
    let ends = crate::loose_ends::loose_ends(&resolved).unwrap();
    assert!(
        ends.iter().all(|end| end.id != "q-aaaaaaaaaaaa"),
        "{ends:#?}"
    );
}

#[test]
fn the_exchange_echo_is_evidence_after_the_struck_twin_was_deleted() {
    // The twin's tree was deleted (🗑️); the exchange still echoes it, and the
    // follow-up that stayed names the commit.
    let queue = format!(
        "{LIVE_COPY}\n- 👀 [#q-bbbbbbbbbbbb] follow-up: agent: check that CI run 45094135922 on `9d07673718` finishes green. [#q-dddddddddddd]\n"
    );
    let document = session(&queue, &exchange_with_echo());
    let resolved = reconcile(&document).expect("the echo proves the twin finished");
    assert!(
        resolved.contains("    - **Summary:** Duplicate of finished work: done by `[#q-bbbbbbbbbbbb]`/`9d07673718`"),
        "{resolved}"
    );

    // With no commit evidence beyond the multi-commit exchange body, the
    // response names the exchange entry instead of guessing a commit.
    let document = session(&format!("{LIVE_COPY}\n"), &exchange_with_echo());
    let resolved = reconcile(&document).unwrap();
    assert!(
        resolved.contains(
            "done by `[#q-bbbbbbbbbbbb]` (agent:exchange “Re: phase 2 · top-level counts · memory”)"
        ),
        "{resolved}"
    );
}

#[test]
fn an_uncertain_duplicate_is_surfaced_not_struck() {
    let twins = twin_rows();
    // Still being typed (not in the committed baseline).
    let document = session(&format!("{LIVE_COPY}\n{twins}"), &exchange_with_echo());
    assert_eq!(
        reconcile_finished_rows(&document, None, &never_in_flight).unwrap(),
        None
    );
    // A claim attempt holds it.
    assert_eq!(
        reconcile_finished_rows(&document, Some(&document), &|text: &str| text
            .contains("key=report]"))
        .unwrap(),
        None
    );
    // Either way the Loose ends entry names the twin.
    let ends = crate::loose_ends::loose_ends(&document).unwrap();
    let entry = ends
        .iter()
        .find(|end| end.id == "q-aaaaaaaaaaaa")
        .expect("the operator-verify copy is a loose end");
    assert!(
        entry
            .text
            .ends_with("(possibly done by `[#q-bbbbbbbbbbbb]`/`9d07673718`)"),
        "{entry:?}"
    );
}

#[test]
fn similarity_alone_never_strikes() {
    let struck_with_response = |text: &str| {
        format!(
            "- ### ~~{text}~~\n  - **Response [#r-111111111111]**\n    - **Summary:** Fixed in 1a2b3c4d5e.\n    - **State:** Complete.\n"
        )
    };
    let cases = [
        // The operator's own prompt, typed again: never struck by similarity.
        (
            "- Fix the flaky login test in the browser suite before release.\n",
            "Fix the flaky login test in the browser suite before release.",
            true,
        ),
        // The same inbound key sent again is new work (the test failed again).
        (
            "- [from:ci key=test/LoginSpec] LoginSpec failed in the browser suite\n",
            "[from:ci key=test/LoginSpec] LoginSpec failed in the browser suite",
            true,
        ),
        // Different builds are different failures, not a re-queue family.
        (
            "- [from:ci key=build-43] build 43 failed\n",
            "[from:ci key=build-42] build 42 failed",
            false,
        ),
        // A re-queued key whose prose differs too much.
        (
            "- [from:chat key=notes2] summarize the release notes for the plugin\n",
            "[from:chat key=notes] draft the migration guide for the config format",
            false,
        ),
    ];
    for (live, twin, possible) in cases {
        let document = session(&format!("{live}{}", struck_with_response(twin)), "");
        assert_eq!(reconcile(&document), None, "{live}");
        let tree = queue_tree(&document, &QueueTreeOptions::default()).unwrap();
        let surfaced = possible_finished_twins(&document, &tree);
        assert_eq!(!surfaced.is_empty(), possible, "{live}: {surfaced:?}");
    }
}

#[test]
fn operator_rows_and_replies_are_never_struck_as_duplicates() {
    let twin = "[from:chat key=ship] operator: approve the release of the plugin build to the marketplace [#q-eeeeeeeeeeee]";
    let live = "- 🙋 [from:chat key=ship2] operator: approve the release of the plugin build to the marketplace [operator-verify] [#q-ffffffffffff]\n";
    let document = session(
        &format!(
            "{live}- ### ~~{twin}~~\n  - **Response [#r-222222222222]**\n    - **State:** Complete.\n"
        ),
        "",
    );
    assert_eq!(reconcile(&document), None);
    // A row with live work beneath it is not struck either.
    let document = session(
        &format!(
            "{LIVE_COPY}\n  - 📥 agent: also update the changelog for the count token. [#q-123456789abc]\n{}",
            twin_rows()
        ),
        "",
    );
    assert_eq!(reconcile(&document), None);
}

const OPERATOR_ROW: &str = "- 🙋 [#q-0d58062d875f] follow-up: operator: in `tasks/x/dev.md`, remove one of the two `do [#pushchanges]` rows (lines 192 `📥` and 353 bare). Then dispatch can re-admit it. [operator-verify] [#q-76f0eded2463]";

#[test]
fn an_operator_completion_reply_resolves_its_operator_verify_row() {
    for reply in ["done", "Done.", "✅", "completed"] {
        let document = session(&format!("{OPERATOR_ROW}\n  - {reply}\n"), "");
        let resolved = reconcile(&document).unwrap_or_else(|| panic!("{reply:?} resolves"));
        let lines = resolved
            .lines()
            .skip_while(|line| !line.contains("q-0d58062d875f"))
            .take(5)
            .collect::<Vec<_>>();
        assert!(
            lines[0].starts_with("- ~~[#q-0d58062d875f] follow-up: operator: in `tasks/x/dev.md`")
                && lines[0].ends_with(&format!(
                    "[#q-76f0eded2463] — operator answered “{reply}” (#opdonereply)~~"
                )),
            "{resolved}"
        );
        assert_eq!(lines[1], format!("  - ~~{reply}~~"));
        assert!(lines[2].starts_with("  - **Response [#r-"), "{resolved}");
        assert_eq!(
            lines[3],
            format!(
                "    - **Summary:** The operator answered “{reply}” under this row: the operator action is done. Un-strike the row if it still needs doing."
            )
        );
        assert_eq!(lines[4], "    - **State:** Complete.");
        assert_eq!(reconcile(&resolved), None, "{resolved}");
        assert!(crate::loose_ends::loose_ends(&resolved).unwrap().is_empty());
    }
}

#[test]
fn a_nested_done_under_an_operator_verify_row_is_not_a_head() {
    let body = format!("{OPERATOR_ROW}\n  - done\n");
    let entries = crate::document_queue::parse(&body).unwrap();
    let heads = crate::document_queue::prompts(&entries)
        .into_iter()
        .map(|prompt| prompt.text.clone())
        .collect::<Vec<_>>();
    assert!(
        heads.iter().all(|text| !is_completion_reply(text)),
        "{heads:?}"
    );
    // The same word under an ordinary agent row is still work.
    let agent = "- [from:chat key=x] agent: rebuild the docs site. [#q-0a0a0a0a0a0a]\n  - done\n";
    let entries = crate::document_queue::parse(agent).unwrap();
    assert!(
        crate::document_queue::prompts(&entries)
            .iter()
            .any(|prompt| is_completion_reply(&prompt.text)),
        "a non-operator parent still offers the nested reply"
    );
}

#[test]
fn a_struck_done_reply_strikes_its_operator_verify_parent() {
    let document = session(&format!("{OPERATOR_ROW}\n  - ~~done~~\n"), "");
    let resolved = reconcile(&document).expect("~~done~~ strikes the parent");
    let lines = resolved
        .lines()
        .skip_while(|line| !line.contains("q-0d58062d875f"))
        .take(5)
        .collect::<Vec<_>>();
    assert!(
        lines[0].starts_with("- ~~[#q-0d58062d875f] follow-up: operator:")
            && lines[0].ends_with("[#q-76f0eded2463] — operator answered “done” (#opdonereply)~~"),
        "{resolved}"
    );
    assert_eq!(lines[1], "  - ~~done~~");
    assert!(lines[2].starts_with("  - **Response [#r-"), "{resolved}");
    assert!(
        lines[3].contains("The operator answered “done” under this row"),
        "{resolved}"
    );
    assert_eq!(lines[4], "    - **State:** Complete.");
    assert_eq!(reconcile(&resolved), None, "{resolved}");
}

#[test]
fn a_struck_done_reply_with_its_response_still_strikes_the_parent() {
    // The reply was dispatched before it could close the parent. Its response
    // stays; the parent still strikes.
    let document = session(
        &format!(
            "{OPERATOR_ROW}\n  - ~~done~~\n    - **Response [#r-aaaaaaaaaaaa]**\n      - **Summary:** BLOCKED already merged.\n      - **State:** Waiting on operator input.\n      - **Guidance:** The row stopped.\n"
        ),
        "",
    );
    let resolved = reconcile(&document).expect("the response does not keep the parent open");
    let parent = live_line(&resolved, "q-0d58062d875f").unwrap();
    assert!(
        parent.contains("operator answered “done” (#opdonereply)~~"),
        "{resolved}"
    );
    assert!(resolved.contains("  - ~~done~~\n"), "{resolved}");
    assert!(
        resolved.contains("The operator answered “done” under this row"),
        "{resolved}"
    );
    assert!(
        resolved.contains("**State:** Waiting on operator input."),
        "{resolved}"
    );
    assert_eq!(reconcile(&resolved), None, "{resolved}");
}

#[test]
fn an_operator_row_is_never_struck_without_a_settled_completion_reply() {
    let cases = [
        // Not a completion reply.
        format!("{OPERATOR_ROW}\n  - done?\n"),
        format!("{OPERATOR_ROW}\n  - done, but the second row is still there\n"),
        format!("{OPERATOR_ROW}\n  - continue\n"),
        // No reply at all.
        format!("{OPERATOR_ROW}\n"),
        // A second live row beneath it.
        format!("{OPERATOR_ROW}\n  - done\n  - 📥 agent: re-admit the row. [#q-abcdefabcdef]\n"),
        // A reply that is itself a parent.
        format!("{OPERATOR_ROW}\n  - done\n    - see the commit\n"),
        // Not an operator-verify row: a plain agent row.
        "- [from:chat key=x] agent: rebuild the docs site. [#q-0a0a0a0a0a0a]\n  - done\n"
            .to_string(),
    ];
    for queue in cases {
        let document = session(&queue, "");
        assert_eq!(reconcile(&document), None, "{queue}");
    }
    // The reply is still being typed.
    let document = session(&format!("{OPERATOR_ROW}\n  - done\n"), "");
    let baseline = session(&format!("{OPERATOR_ROW}\n"), "");
    assert_eq!(
        reconcile_finished_rows(&document, Some(&baseline), &never_in_flight).unwrap(),
        None
    );
}

#[test]
fn commit_tokens_skip_ids_and_run_numbers() {
    assert_eq!(
        commit_shas("on `9d07673718`, run 45094135922 [#q-0edb8bc4fdf6] r-2500918c17d7 deadbeef"),
        vec!["9d07673718".to_string()]
    );
    assert!(requeue_family("report", "report2"));
    assert!(!requeue_family("report2", "report2"));
    assert!(!requeue_family("build-42", "build-43"));
    assert!(!requeue_family("report2", "report3"));
}

#[test]
fn a_bare_struck_twin_is_no_evidence() {
    // Struck without a response and never echoed: it may have been cancelled.
    let document = session(&format!("{LIVE_COPY}\n- ### ~~{TWIN_TEXT}~~\n"), "");
    assert_eq!(reconcile(&document), None);
    let tree = queue_tree(&document, &QueueTreeOptions::default()).unwrap();
    assert!(possible_finished_twins(&document, &tree).is_empty());
}

#[test]
fn ack_text_resolves_exactly_one_completion_reply() {
    let document = session(
        &format!("{OPERATOR_ROW}\n  - done\n- keep this queued\n"),
        "",
    );
    match resolve_completion_reply_ack(&document, "done", &never_in_flight, None).unwrap() {
        CompletionReplyAck::Resolved { document, parent } => {
            assert!(
                parent.starts_with("[#q-0d58062d875f] follow-up: operator:"),
                "{parent}"
            );
            assert!(
                document.contains("  - ~~done~~\n  - **Response [#r-"),
                "{document}"
            );
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        resolve_completion_reply_ack(&document, "keep this queued", &never_in_flight, None)
            .unwrap(),
        CompletionReplyAck::None
    );
    let second = OPERATOR_ROW.replace("q-0d58062d875f", "q-111111111111");
    let both = session(
        &format!("{OPERATOR_ROW}\n  - done\n{second}\n  - done\n"),
        "",
    );
    assert!(matches!(
        resolve_completion_reply_ack(&both, "done", &never_in_flight, None).unwrap(),
        CompletionReplyAck::Ambiguous(parents) if parents.len() == 2
    ));
}

#[test]
fn a_painted_completion_reply_is_still_the_operators_answer() {
    // The reply was admitted as a leaf and painted `📥` after the baseline.
    let baseline = session(&format!("{OPERATOR_ROW}\n  - done\n"), "");
    let document = session(&format!("{OPERATOR_ROW}\n  - 📥 done\n"), "");
    let resolved = reconcile_finished_rows(&document, Some(&baseline), &never_in_flight)
        .unwrap()
        .expect("the painted reply resolves its row");
    assert!(
        resolved.contains("“done” (#opdonereply)~~\n  - ~~done~~\n"),
        "{resolved}"
    );
    // A reply a worker attempt holds is not the operator's answer yet.
    let claimed = session(
        &format!("{OPERATOR_ROW}\n  - 🚧 [#subagent:x#1] done\n"),
        "",
    );
    assert_eq!(
        reconcile_finished_rows(&claimed, Some(&claimed), &never_in_flight).unwrap(),
        None
    );
}
