//! `agent:worktrees` is the managed-list inventory before `agent:recap`.

use agent_doc_work_projection::worktrees::{
    ClaimSignal, PullRequestState, RowSignal, WorkStatus, WorktreeCandidate, WorktreeOperatorChild,
    WorktreeOperatorChildKind, WorktreeRecord, active_records, auto_prune,
    combine_pull_request_states, named_pull_requests, operator_child_actions,
    parse_pull_request_state, promote_worktree_instructions, reconcile, row_signal, work_status,
};

fn record(topic: &str, branch: &str, directory: &str) -> WorktreeRecord {
    WorktreeRecord {
        topic: topic.to_string(),
        branch: branch.to_string(),
        directory: directory.to_string(),
    }
}

const DOC: &str = concat!(
    "## Status <!-- agent:status patch=replace -->\n",
    "<!-- /agent:status -->\n",
    "\n## Recap <!-- agent:recap patch=replace -->\n",
    "<!-- /agent:recap -->\n",
    "\n## Active <!-- agent:active priority -->\n",
    "<!-- /agent:active -->\n",
    "\n## Run <!-- agent:run -->\n",
    "<!-- /agent:run -->\n",
);

#[test]
fn inventory_lands_before_recap_and_is_a_fixed_point() {
    let records = vec![
        record("Ship the list", "feat/worktrees", "/tmp/wt-b"),
        record("Earlier topic", "feat/earlier", "/tmp/wt-a"),
    ];
    let updated = reconcile(DOC, &records)
        .unwrap()
        .expect("a live worktree inserts the section");
    let status = updated.find("<!-- /agent:status -->").unwrap();
    let worktrees = updated.find("<!-- agent:worktrees -->").unwrap();
    let recap = updated.find("<!-- agent:recap patch=replace -->").unwrap();
    let active = updated.find("<!-- agent:active priority -->").unwrap();
    assert!(
        status < worktrees && worktrees < recap && recap < active,
        "agent:worktrees sits before agent:recap:\n{updated}"
    );
    assert!(
        updated.contains(concat!(
            "- **Topic:** Earlier topic\n",
            "  - **Branch:** feat/earlier\n",
            "  - **Directory:** `/tmp/wt-a`\n",
            "- **Topic:** Ship the list\n",
            "  - **Branch:** feat/worktrees\n",
            "  - **Directory:** `/tmp/wt-b`\n",
        )),
        "{updated}"
    );
    let noted = updated.replace(
        "  - **Directory:** `/tmp/wt-a`\n",
        "  - **Directory:** `/tmp/wt-a`\n  - operator note\n",
    );
    assert!(
        reconcile(&noted, &records).unwrap().is_none(),
        "an operator child under a live directory is kept"
    );
    assert!(noted.contains("  - operator note\n"));
    let cleared = reconcile(&noted, &[]).unwrap().expect("stale rows clear");
    assert!(
        !cleared.contains("Earlier topic"),
        "a directory that is no longer live leaves the inventory:\n{cleared}"
    );
    assert!(
        !cleared.contains("operator note"),
        "a note leaves with its directory"
    );
    assert!(reconcile(&cleared, &[]).unwrap().is_none());
    assert!(
        reconcile(DOC, &[]).unwrap().is_none(),
        "no live worktree does not insert an empty section into an old document"
    );
}

#[test]
fn active_records_drop_removed_and_duplicate_directories() {
    let candidates = vec![
        WorktreeCandidate {
            topic: "first\nline two".to_string(),
            branch: "feat/one".to_string(),
            directory: "/wt/same".to_string(),
            generation: 1,
            active: true,
        },
        WorktreeCandidate {
            topic: "second".to_string(),
            branch: String::new(),
            directory: "/wt/same".to_string(),
            generation: 2,
            active: true,
        },
        WorktreeCandidate {
            topic: "removed".to_string(),
            branch: "feat/gone".to_string(),
            directory: "/wt/gone".to_string(),
            generation: 9,
            active: false,
        },
    ];
    assert_eq!(
        active_records(&candidates),
        vec![WorktreeRecord {
            topic: "second".to_string(),
            branch: "detached".to_string(),
            directory: "/wt/same".to_string(),
        }]
    );
}

#[test]
fn topic_names_pull_requests_and_closed_means_merged_too() {
    let named = named_pull_requests(
        "https://github.com/haiven-dev/haiven-api/pull/837: #rebase\nnot a topic line",
    );
    assert_eq!(named.len(), 1);
    assert_eq!(named[0].owner, "haiven-dev");
    assert_eq!(named[0].repo, "haiven-api");
    assert_eq!(named[0].number, 837);
    assert!(
        named_pull_requests("https://github.com/haiven-dev/haiven-sdk-unity: Create a PR")
            .is_empty()
    );
    assert_eq!(
        parse_pull_request_state("{\"state\":\"MERGED\"}\n"),
        PullRequestState::Closed
    );
    assert_eq!(parse_pull_request_state("OPEN"), PullRequestState::Open);
    assert_eq!(parse_pull_request_state("nope"), PullRequestState::Unknown);
    assert_eq!(
        combine_pull_request_states(&[PullRequestState::Closed, PullRequestState::Open]),
        Some(PullRequestState::Open)
    );
}

#[test]
fn prune_keeps_an_open_pull_request_and_incomplete_work() {
    assert!(auto_prune(
        Some(PullRequestState::Closed),
        WorkStatus::Unknown
    ));
    assert!(auto_prune(None, WorkStatus::Complete));
    assert!(!auto_prune(
        Some(PullRequestState::Open),
        WorkStatus::Complete
    ));
    assert!(!auto_prune(
        Some(PullRequestState::Unknown),
        WorkStatus::Complete
    ));
    assert!(!auto_prune(
        Some(PullRequestState::Closed),
        WorkStatus::Incomplete
    ));
    assert!(!auto_prune(None, WorkStatus::Unknown));
    assert_eq!(
        work_status(false, ClaimSignal::InProgress, RowSignal::Absent),
        WorkStatus::Incomplete
    );
    assert_eq!(
        work_status(false, ClaimSignal::Reported, RowSignal::Absent),
        WorkStatus::Complete
    );
    assert_eq!(
        work_status(true, ClaimSignal::None, RowSignal::Live),
        WorkStatus::Incomplete
    );
    assert_eq!(
        work_status(false, ClaimSignal::None, RowSignal::Struck),
        WorkStatus::Complete
    );
}

#[test]
fn a_live_queue_row_is_incomplete_and_a_struck_row_is_done() {
    let live = concat!(
        "## Run <!-- agent:run -->\n",
        "- https://github.com/example/repo/pull/9: still working\n",
        "<!-- /agent:run -->\n",
        "## Worktrees <!-- agent:worktrees -->\n",
        "- **Topic:** https://github.com/example/repo/pull/9: still working\n",
        "  - **Directory:** `/tmp/wt`\n",
        "<!-- /agent:worktrees -->\n",
    );
    assert_eq!(
        row_signal(
            live,
            "https://github.com/example/repo/pull/9: still working"
        )
        .unwrap(),
        RowSignal::Live
    );
    let struck = live.replace(
        "- https://github.com/example/repo/pull/9: still working\n",
        "- ~~https://github.com/example/repo/pull/9: still working~~\n",
    );
    assert_eq!(
        row_signal(
            &struck,
            "https://github.com/example/repo/pull/9: still working"
        )
        .unwrap(),
        RowSignal::Struck
    );
}

#[test]
fn operator_list_children_are_actions_and_a_note_is_not() {
    let body = concat!(
        "- **Topic:** https://github.com/example/repo/pull/4: rebase\n",
        "  - **Branch:** feat/rebase\n",
        "  - **Directory:** `/tmp/wt-a`\n",
        "  - delete\n",
        "  - merge into sdk-dev\n",
        "  plain note\n",
        "- **Topic:** finished notes for the sdk document\n",
        "  - **Branch:** docs/notes\n",
        "  - **Directory:** `/tmp/wt-b`\n",
        "  - [x] merge\n",
        "  - remove the worktree\n",
    );
    assert_eq!(
        operator_child_actions(body),
        vec![
            WorktreeOperatorChild {
                directory: "/tmp/wt-a".into(),
                branch: "feat/rebase".into(),
                topic: "https://github.com/example/repo/pull/4: rebase".into(),
                kind: WorktreeOperatorChildKind::Remove,
            },
            WorktreeOperatorChild {
                directory: "/tmp/wt-a".into(),
                branch: "feat/rebase".into(),
                topic: "https://github.com/example/repo/pull/4: rebase".into(),
                kind: WorktreeOperatorChildKind::Instruction("merge into sdk-dev".into()),
            },
            WorktreeOperatorChild {
                directory: "/tmp/wt-b".into(),
                branch: "docs/notes".into(),
                topic: "finished notes for the sdk document".into(),
                kind: WorktreeOperatorChildKind::Remove,
            },
        ]
    );
}

#[test]
fn a_merge_instruction_is_queued_for_the_agent_and_not_left_inert() {
    let doc = concat!(
        "## Worktrees <!-- agent:worktrees -->\n",
        "- **Topic:** Ship the list\n",
        "  - **Branch:** feat/worktrees\n",
        "  - **Directory:** `/tmp/wt-b`\n",
        "  - merge into main\n",
        "  plain note\n",
        "<!-- /agent:worktrees -->\n",
        "## Run <!-- agent:run id=main -->\n",
        "- existing work\n",
        "<!-- /agent:run -->\n",
    );
    let promoted = promote_worktree_instructions(doc, "main")
        .unwrap()
        .expect("merge is an action");
    assert_eq!(promoted.inserted, 1);
    assert!(
        promoted.content.contains("- ### existing work\n"),
        "the existing queue row stays, with the binary-owned heading:\n{}",
        promoted.content
    );
    assert!(
        promoted
            .content
            .contains("Perform the operator instruction on worktree `/tmp/wt-b`")
    );
    assert!(promoted.content.contains("merge into main"));
    assert!(
        !promoted.content.contains("  - merge into main\n"),
        "the instruction child is consumed:\n{}",
        promoted.content
    );
    assert!(
        promoted.content.contains("  plain note\n"),
        "a non-list note stays"
    );
    assert!(
        promote_worktree_instructions(&promoted.content, "main")
            .unwrap()
            .is_none(),
        "a second pass does not queue the instruction again"
    );
}

#[test]
fn delete_wins_over_a_merge_instruction_on_the_same_topic() {
    let doc = concat!(
        "## Worktrees <!-- agent:worktrees -->\n",
        "- **Topic:** Ship the list\n",
        "  - **Branch:** feat/worktrees\n",
        "  - **Directory:** `/tmp/wt-b`\n",
        "  - merge into main\n",
        "  - delete\n",
        "<!-- /agent:worktrees -->\n",
        "## Run <!-- agent:run id=main -->\n",
        "<!-- /agent:run -->\n",
    );
    assert!(
        promote_worktree_instructions(doc, "main")
            .unwrap()
            .is_none(),
        "a removal child keeps the merge instruction off the queue"
    );
}
