//! Nested sub-prompts in list components (`#nestedsubprompts`).
//!
//! A row with nested rows is context rather than executable work. Its leaf
//! descendants are independent queue heads, inherit their ancestor context,
//! and remain executable even when the parent is blocked or struck.

use std::fs;
use std::process::Command;

/// `#integrationtesttargets`: these commands reach the project controller,
/// which they launch as production does.
const CONTROLLER_LAUNCH_REASON: &str =
    "nested sub-prompt CLI fixture drives queue claim/brief/preflight against a temp document";

const PARENT: &str = "Sync PR and Jira state into this document.";

fn git(root: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_agent_doc(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_agent-doc"))
        .env(
            agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
            CONTROLLER_LAUNCH_REASON,
        )
        .env("AGENT_DOC_ROOT_CEILING_DIRECTORIES", dir.parent().unwrap())
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn assert_ok(output: &std::process::Output, what: &str) {
    assert!(
        output.status.success(),
        "{what} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A committed, baselined session whose runnable list is spelled `kind`
/// (`run`, `queue` or `loop`), plus tracked checklist components whose items
/// carry nested rows.
fn committed_session(dir: &std::path::Path, kind: &str, run_rows: &str) -> std::path::PathBuf {
    fs::create_dir_all(dir.join(".agent-doc/locks")).unwrap();
    let doc = dir.join("session.md");
    let content = format!(
        concat!(
            "---\nagent_doc_session: nested-subprompts\nagent_doc_format: template\n",
            "prompt_presets:\n  \"#auth\": \"I authorize you to do this.\"\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append subagents -->\n",
            "### Re: dispatch — gpt-5 · 2026-10-27T08:00-04:00\n\nReady.\n",
            "<!-- agent:boundary:keep -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Run <!-- agent:{kind} subagents preset=\"#auth\" go -->\n",
            "{rows}",
            "<!-- /agent:{kind} -->\n\n",
            "## Backlog <!-- agent:backlog queue -->\n",
            "- [ ] [#alpha] fix the alpha defect\n",
            "  - [ ] [#alpha-logs] check the alpha logs\n",
            "  - [ ] [#alpha-test] add a regression test\n",
            "<!-- /agent:backlog -->\n\n",
            "## Review <!-- agent:review -->\n",
            "- [ ] [#rev1] review the beta change\n",
            "  - focus on the merge path\n",
            "<!-- /agent:review -->\n\n",
            "## Todo <!-- agent:todo -->\n",
            "- [ ] call the vendor\n",
            "  - ask about the rate limit\n",
            "<!-- /agent:todo -->\n\n",
            "## Icebox <!-- agent:icebox -->\n",
            "- [ ] [#later] someday item\n",
            "  - [ ] [#later-note] with a nested note\n",
            "<!-- /agent:icebox -->\n",
        ),
        kind = kind,
        rows = run_rows,
    );
    fs::write(&doc, &content).unwrap();
    git(dir, &["init", "-q"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "user.name", "Test User"]);
    git(dir, &["add", "session.md"]);
    git(dir, &["commit", "-q", "--no-verify", "-m", "initial"]);
    agent_doc_snapshot_io::checkpoint_document_baseline(
        &doc,
        &content,
        agent_doc_ops_log_io::log_op,
    )
    .unwrap();
    doc
}

fn preflight_json(dir: &std::path::Path, doc: &std::path::Path) -> serde_json::Value {
    let output = run_agent_doc(dir, &["preflight", doc.to_str().unwrap()]);
    assert_ok(&output, "preflight");
    serde_json::from_slice(&output.stdout).unwrap_or_else(|err| {
        panic!(
            "preflight stdout is not JSON ({err}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

fn string_list(state: &serde_json::Value, key: &str) -> Vec<String> {
    state
        .get(key)
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .map(|value| match value {
                    serde_json::Value::String(text) => text.clone(),
                    other => other
                        .get("item")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| other.to_string()),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The body of the runnable list component, as written on disk.
fn run_body(doc: &std::path::Path, kind: &str) -> String {
    let content = fs::read_to_string(doc).unwrap();
    let open = format!("<!-- agent:{kind} ");
    let start = content.find(&open).unwrap();
    let body_start = start + content[start..].find('\n').unwrap() + 1;
    let end = content.find(&format!("<!-- /agent:{kind} -->")).unwrap();
    content[body_start..end].to_string()
}

const CHILDREN: [&str; 4] = [
    "So this is the first subtask.",
    "This is the second subtask.",
    "Subtasks should be editable and steerable.",
    "Make sure these subtasks are being deterministically associated with the parent task.",
];

/// The live shape: a parent with four children, the first child still carrying
/// the per-child projection the pre-fix binary stamped on it.
fn live_rows() -> String {
    format!(
        "- 📥 {PARENT}\n  - 🟡 [#subagent:pmsync#1] {}\n  - {}\n  - {}\n  - {}\n- separate flat head\n",
        CHILDREN[0], CHILDREN[1], CHILDREN[2], CHILDREN[3]
    )
}

#[test]
fn only_nested_leaves_run_in_every_runnable_list_spelling() {
    for kind in ["run", "queue", "loop"] {
        let dir = tempfile::tempdir().unwrap();
        let doc = committed_session(dir.path(), kind, &live_rows());
        let state = preflight_json(dir.path(), &doc);

        let prompts = string_list(&state, "queue_prompts");
        assert!(
            prompts.iter().all(|item| item != PARENT),
            "{kind}: the parent is context-only: {prompts:#?}"
        );
        for child in CHILDREN {
            assert!(
                prompts.iter().any(|item| item == child),
                "{kind}: leaf {child:?} is executable: {prompts:#?}"
            );
        }
        assert!(
            prompts.iter().any(|item| item == "separate flat head"),
            "{kind}: the flat sibling stays executable: {prompts:#?}"
        );

        let dispatch = string_list(&state, "queue_subagent_dispatch");
        assert!(
            dispatch.iter().all(|item| item != PARENT),
            "{kind}: the parent is never dispatched: {dispatch:#?}"
        );
        for child in CHILDREN {
            assert!(
                dispatch.iter().any(|item| item == child),
                "{kind}: every leaf is independently dispatchable: {dispatch:#?}"
            );
        }
        assert_eq!(
            dispatch.iter().any(|item| item == "separate flat head"),
            kind != "loop",
            "{kind}: the flat sibling follows the component policy: {dispatch:#?}"
        );

        // Tracked checklist rows keep their children in the item, never in the
        // runnable list.
        let body = run_body(&doc, kind);
        for nested in [
            "check the alpha logs",
            "add a regression test",
            "focus on the merge path",
            "ask about the rate limit",
            "with a nested note",
        ] {
            assert!(
                !body.contains(nested),
                "{kind}: tracked child {nested:?} leaked into the runnable list:\n{body}"
            );
        }
    }
}
#[test]
fn a_leaf_is_claimable_and_its_brief_carries_parent_context() {
    let dir = tempfile::tempdir().unwrap();
    let doc = committed_session(dir.path(), "run", &live_rows());
    let path = doc.to_str().unwrap();

    let parent_brief = run_agent_doc(dir.path(), &["queue", "brief", path, "--item", PARENT]);
    assert!(
        !parent_brief.status.success(),
        "a context parent must not resolve as a live queue head"
    );

    let brief = run_agent_doc(dir.path(), &["queue", "brief", path, "--item", CHILDREN[0]]);
    assert_ok(&brief, "queue brief for the leaf");
    let brief = String::from_utf8_lossy(&brief.stdout).to_string();
    let lineage = format!("{PARENT}: {}", CHILDREN[0]);
    assert!(
        brief.contains(&format!("> {lineage}\n")),
        "the submitted prompt carries parent lineage: {brief}"
    );
    for child in CHILDREN {
        assert!(
            brief.contains(child),
            "the prompt tree carries {child:?}:\n{brief}"
        );
    }

    assert_ok(
        &run_agent_doc(
            dir.path(),
            &[
                "queue",
                "claim",
                path,
                "--item",
                CHILDREN[0],
                "--owner",
                "subagent:pmsync",
            ],
        ),
        "queue claim on the leaf",
    );
    let body = run_body(&doc, "run");
    assert!(
        body.starts_with(&format!(
            "- {PARENT}\n  - 🟡 [#subagent:pmsync#1] {}\n",
            CHILDREN[0]
        )),
        "only the claimed leaf carries lifecycle projection:\n{body}"
    );

    let state = preflight_json(dir.path(), &doc);
    let dispatch = string_list(&state, "queue_subagent_dispatch");
    assert!(
        dispatch.iter().all(|item| item != CHILDREN[0]),
        "the claimed leaf stays out of dispatch: {dispatch:#?}"
    );

    let claims = run_agent_doc(dir.path(), &["queue", "claims", path]);
    assert_ok(&claims, "queue claims");
    let claims: serde_json::Value = serde_json::from_slice(&claims.stdout).unwrap();
    let claims = claims.as_array().unwrap();
    assert_eq!(claims.len(), 1, "{claims:#?}");
    assert_eq!(claims[0]["owner"], "subagent:pmsync");
}

#[test]
fn a_struck_parent_does_not_retire_its_leaves() {
    let dir = tempfile::tempdir().unwrap();
    let rows = format!(
        "- ~~{PARENT}~~\n  - {}\n  - {}\n- separate flat head\n",
        CHILDREN[0], CHILDREN[1]
    );
    let doc = committed_session(dir.path(), "run", &rows);
    let state = preflight_json(dir.path(), &doc);
    let prompts = string_list(&state, "queue_prompts");
    let dispatch = string_list(&state, "queue_subagent_dispatch");
    assert!(
        prompts.iter().all(|item| item != PARENT) && dispatch.iter().all(|item| item != PARENT),
        "the struck parent is context-only: {prompts:#?} {dispatch:#?}"
    );
    for child in [CHILDREN[0], CHILDREN[1]] {
        assert!(prompts.iter().any(|item| item == child), "{prompts:#?}");
        assert!(dispatch.iter().any(|item| item == child), "{dispatch:#?}");
    }
}
/// A live row added beneath a struck parent is an executable leaf. The struck
/// parent and answered sibling are context, not a reopened compound head.
#[test]
fn a_row_added_under_a_struck_parent_is_dispatched_as_a_leaf() {
    let dir = tempfile::tempdir().unwrap();
    let parent = "#subagent-operator-request-handoff-integration-id";
    let answered = "Describe the choices in more detail. Why do we need a SBX/STG id?";
    let added = "I don't know if you understand that we moved onto project templates.";
    let closed = format!("- ~~{parent}~~\n  - ~~{answered}~~\n- separate flat head\n");
    let doc = committed_session(dir.path(), "run", &closed);
    let path = doc.to_str().unwrap();

    let content = fs::read_to_string(&doc).unwrap();
    fs::write(
        &doc,
        content.replace(
            &format!("- ~~{parent}~~\n  - ~~{answered}~~\n"),
            &format!("- ~~{parent}~~\n  - ~~{answered}~~\n  - {added}\n"),
        ),
    )
    .unwrap();

    let state = preflight_json(dir.path(), &doc);
    let prompts = string_list(&state, "queue_prompts");
    let dispatch = string_list(&state, "queue_subagent_dispatch");
    assert!(prompts.iter().any(|item| item == added), "{prompts:#?}");
    assert!(dispatch.iter().any(|item| item == added), "{dispatch:#?}");
    assert!(dispatch.iter().all(|item| item != parent), "{dispatch:#?}");

    let brief = run_agent_doc(
        dir.path(),
        &["queue", "brief", path, "--item", added, "--json"],
    );
    assert_ok(&brief, "queue brief on the added leaf");
    let brief: serde_json::Value = serde_json::from_slice(&brief.stdout).unwrap();
    assert_eq!(brief["item"], format!("{parent}: {added}"));
    let preamble = brief["subagent_prompt_preamble"].as_str().unwrap();
    assert!(preamble.contains(parent), "{preamble}");
    assert!(preamble.contains(answered), "{preamble}");

    assert_ok(
        &run_agent_doc(
            dir.path(),
            &[
                "queue",
                "claim",
                path,
                "--item",
                added,
                "--owner",
                "subagent:reopen",
            ],
        ),
        "queue claim on the added leaf",
    );
    assert!(
        run_body(&doc, "run").contains(&format!("  - 🟡 [#subagent:reopen#1] {added}\n")),
        "{}",
        run_body(&doc, "run")
    );
}

/// A struck context parent stays terminal without projecting completion onto
/// its independently live leaf.
#[test]
fn a_struck_parent_over_an_unstruck_child_keeps_the_child_live() {
    let dir = tempfile::tempdir().unwrap();
    let rows = format!(
        "- ~~{PARENT}~~\n  - {}\n- separate flat head\n",
        CHILDREN[0]
    );
    let doc = committed_session(dir.path(), "run", &rows);
    let state = preflight_json(dir.path(), &doc);
    let dispatch = string_list(&state, "queue_subagent_dispatch");
    assert!(
        dispatch.iter().any(|item| item == CHILDREN[0]),
        "{dispatch:#?}"
    );
    assert!(
        run_body(&doc, "run")
            .starts_with(&format!("- ~~{PARENT}~~\n  - 📥 {}\n", CHILDREN[0])),
        "{}",
        run_body(&doc, "run")
    );
}

#[test]
fn tracked_children_keep_the_group_shape() {
    let dir = tempfile::tempdir().unwrap();
    let doc = committed_session(
        dir.path(),
        "run",
        "- alpha epic\n  - do [#alpha]\n  - remember the logs\n",
    );
    let state = preflight_json(dir.path(), &doc);
    let dispatch = string_list(&state, "queue_subagent_dispatch");
    assert!(
        dispatch.iter().any(|item| item == "do [#alpha]"),
        "a tracked child stays its own head (`#f1s3` group): {dispatch:#?}"
    );
    assert!(
        dispatch.iter().any(|item| item == "remember the logs"),
        "a unique nested leaf is dispatched as that leaf; its lineage stays on the submitted prompt: {dispatch:#?}"
    );
    assert!(
        dispatch.iter().all(|item| item != "alpha epic"),
        "a group label is never executable: {dispatch:#?}"
    );
}

#[test]
fn a_tracked_item_brief_carries_its_nested_rows() {
    let dir = tempfile::tempdir().unwrap();
    let doc = committed_session(dir.path(), "run", "- do [#alpha]\n- do [#rev1]\n");
    let path = doc.to_str().unwrap();
    for (item, nested) in [
        (
            "#alpha",
            &["check the alpha logs", "add a regression test"][..],
        ),
        ("#rev1", &["focus on the merge path"][..]),
    ] {
        let brief = run_agent_doc(
            dir.path(),
            &["queue", "brief", path, "--item", item, "--json"],
        );
        assert_ok(&brief, "queue brief for a tracked item");
        let brief: serde_json::Value = serde_json::from_slice(&brief.stdout).unwrap();
        let text = brief["tracked_items"][0]["text"]
            .as_str()
            .unwrap_or_default();
        for child in nested {
            assert!(text.contains(child), "{item}: {brief:#}");
        }
    }
}

/// A committed, baselined session holding only an exchange, an `agent:run`
/// list with `run_rows`, and an `agent:todo` with `todo_rows`: the closeout
/// shape of the live `agent-doc-bugs.md` run queue (`#nestedechostrike`).
fn closeout_session(dir: &std::path::Path, run_rows: &str, todo_rows: &str) -> std::path::PathBuf {
    fs::create_dir_all(dir.join(".agent-doc/locks")).unwrap();
    let doc = dir.join("session.md");
    let content = format!(
        concat!(
            "---\nagent_doc_session: nested-echo-strike\nagent_doc_format: template\n",
            "prompt_presets:\n  \"#auth\": \"I authorize you to do this.\"\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append subagents -->\n",
            "### Re: dispatch — gpt-5 · 2026-10-27T08:00-04:00\n\nReady.\n",
            "<!-- agent:boundary:keep -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Run <!-- agent:run subagents preset=\"#auth\" go -->\n",
            "{run_rows}",
            "<!-- /agent:run -->\n\n",
            "## Todo <!-- agent:todo -->\n",
            "{todo_rows}",
            "<!-- /agent:todo -->\n",
        ),
        run_rows = run_rows,
        todo_rows = todo_rows,
    );
    fs::write(&doc, &content).unwrap();
    git(dir, &["init", "-q"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "user.name", "Test User"]);
    git(dir, &["add", "session.md"]);
    git(dir, &["commit", "-q", "--no-verify", "-m", "initial"]);
    agent_doc_snapshot_io::checkpoint_document_baseline(
        &doc,
        &content,
        agent_doc_ops_log_io::log_op,
    )
    .unwrap();
    doc
}

fn run_agent_doc_stdin(dir: &std::path::Path, args: &[&str], stdin: &str) -> std::process::Output {
    use std::io::Write;
    let mut child = Command::new(env!("CARGO_BIN_EXE_agent-doc"))
        .env(
            agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
            CONTROLLER_LAUNCH_REASON,
        )
        .env("AGENT_DOC_ROOT_CEILING_DIRECTORIES", dir.parent().unwrap())
        .env("AGENT_DOC_SESSION_CHECK_SETTLE_SECS", "1")
        .current_dir(dir)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

/// Claim, start and report one item the way a coordinator dispatches it.
fn dispatch_and_report(dir: &std::path::Path, path: &str, item: &str, owner: &str) {
    assert_ok(
        &run_agent_doc(
            dir,
            &["queue", "claim", path, "--item", item, "--owner", owner],
        ),
        "queue claim",
    );
    assert_ok(
        &run_agent_doc(
            dir,
            &[
                "queue",
                "start",
                path,
                "--item",
                item,
                "--owner",
                owner,
                "--generation",
                "1",
                "--worker-id",
                "w1",
            ],
        ),
        "queue start",
    );
    assert_ok(
        &run_agent_doc(
            dir,
            &[
                "queue",
                "report-complete",
                path,
                "--item",
                item,
                "--generation",
                "1",
                "--response",
                "Part 1 shipped; Part 2 is a design doc.",
            ],
        ),
        "queue report-complete",
    );
}

fn claim_owners(dir: &std::path::Path, path: &str) -> Vec<String> {
    let claims = run_agent_doc(dir, &["queue", "claims", path]);
    assert_ok(&claims, "queue claims");
    let claims: serde_json::Value = serde_json::from_slice(&claims.stdout).unwrap();
    claims
        .as_array()
        .unwrap()
        .iter()
        .map(|claim| claim["owner"].as_str().unwrap_or_default().to_string())
        .collect()
}

/// A full-tree echo can answer every executable leaf, while the parent remains
/// context and a reported leaf's claim ends with that leaf's strike.
#[test]
fn a_verbatim_nested_echo_closes_the_reported_leaf_not_the_parent() {
    let dir = tempfile::tempdir().unwrap();
    let rows = format!(
        "- {PARENT}\n  - {}\n  - {}\n  - {}\n  - {}\n- separate flat head\n",
        CHILDREN[0], CHILDREN[1], CHILDREN[2], CHILDREN[3]
    );
    let doc = closeout_session(dir.path(), &rows, "- [ ] call the vendor\n");
    let path = doc.to_str().unwrap();
    dispatch_and_report(dir.path(), path, CHILDREN[0], "subagent:pmsync");
    assert!(
        run_body(&doc, "run").contains(&format!("  - ☑️ [#subagent:pmsync#1] {}\n", CHILDREN[0])),
        "{}",
        run_body(&doc, "run")
    );

    preflight_json(dir.path(), &doc);
    let echo: String = CHILDREN
        .iter()
        .map(|child| format!(">   - {child}\n"))
        .collect();
    let response = format!(
        "<!-- patch:exchange -->\n### Re: PM sync — gpt-5\n\n> **Queue prompt:** {PARENT}\n{echo}\nPart 1 shipped; Part 2 is a design doc.\n<!-- /patch:exchange -->\n"
    );
    let output = run_agent_doc_stdin(dir.path(), &["respond", path, "--force-disk"], &response);
    assert_ok(&output, "respond");

    let body = run_body(&doc, "run");
    assert!(!body.contains(&format!("- ~~{PARENT}~~")), "{body}");
    assert!(
        body.contains(&format!("  - ~~{}~~", CHILDREN[0])),
        "the reported leaf closes independently:\n{body}"
    );
    for child in &CHILDREN[1..] {
        assert!(
            body.contains(child) && !body.contains(&format!("~~{child}~~")),
            "an unselected sibling stays live:\n{body}"
        );
    }
    assert!(!body.contains("☑️"), "{body}");
    assert!(claim_owners(dir.path(), path).is_empty());
}
/// `#nestedechostrike` (the ahfe row): a reported `do [#id]` dispatch row whose
/// id is an open Todo awaiting an operator check closes with
/// `queue consume --ack-id`, which leaves the Todo open; `--done` would
/// resolve it. The same command refuses while the attempt is still running.
#[test]
fn ack_id_closes_a_reported_todo_backed_row_and_keeps_the_todo_open() {
    let dir = tempfile::tempdir().unwrap();
    let doc = closeout_session(
        dir.path(),
        "- do [#ahfe]\n- separate flat head\n",
        "- [ ] [#ahfe] live-check the IDE\n",
    );
    let path = doc.to_str().unwrap();

    let refused = run_agent_doc(dir.path(), &["queue", "consume", path, "--ack-id", "ahfe"]);
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("never dispatched"),
        "an undispatched Todo row is refused: {}",
        String::from_utf8_lossy(&refused.stderr)
    );

    dispatch_and_report(dir.path(), path, "#ahfe", "subagent:ahfe");
    let closed = run_agent_doc(dir.path(), &["queue", "consume", path, "--ack-id", "ahfe"]);
    assert_ok(&closed, "queue consume --ack-id on the reported row");
    let after = fs::read_to_string(&doc).unwrap();
    let body = run_body(&doc, "run");
    assert!(body.starts_with("- ~~do [#ahfe]~~"), "{body}");
    assert!(body.contains("separate flat head\n"), "{body}");
    assert!(
        after.contains("- [ ] [#ahfe] live-check the IDE\n"),
        "{after}"
    );
    assert!(claim_owners(dir.path(), path).is_empty());
}

/// `#queueoperatoranswer` (live, agent-doc-bugs.md 2026-10-28): the request id the
/// coordinator's operator-request packet minted, and the queue row it paused.
const REQUEST_ID: &str = "subagent-operator-request-undo-agent-writes";
const PAUSED_ROW: &str =
    "Find why a stale replica replay is treated as an operator edit and stop it.";
const ANSWER: &str = "I want sync to use the latest state...even if the operator reverted.";

fn answer_row_todo(state: char) -> String {
    format!(
        "- [{state}] [#{REQUEST_ID}] Answer the blocking subagent question for queue item ‘{PAUSED_ROW}’ in agent:exchange: may the binary rewrite an operator undo? (choices: resync, keep)\n"
    )
}

/// A live answer leaf beneath a request-id context parent answers the request.
/// It remains a distinct queue entry, but is not offered as unrelated dispatch
/// while the live work row still carries the same request id.
#[test]
fn an_answer_row_nested_under_its_request_id_answers_the_request() {
    let dir = tempfile::tempdir().unwrap();
    let rows = format!(
        "- 🙋 {PAUSED_ROW} [#{REQUEST_ID}]\n- separate flat head\n- [#{REQUEST_ID}]\n  - {ANSWER}\n"
    );
    let doc = closeout_session(dir.path(), &rows, &answer_row_todo(' '));

    let state = preflight_json(dir.path(), &doc);
    let prompts = string_list(&state, "queue_prompts");
    let dispatch = string_list(&state, "queue_subagent_dispatch");
    assert!(
        prompts.iter().any(|item| item == ANSWER),
        "the answer is an executable leaf: {prompts:#?}"
    );
    assert!(
        dispatch.iter().any(|item| item == ANSWER),
        "the answer leaf remains executable: {dispatch:#?}"
    );
    assert!(
        dispatch.iter().any(|item| item == "separate flat head"),
        "{dispatch:#?}"
    );
    let body = run_body(&doc, "run");
    assert!(
        body.contains(&format!("[#{REQUEST_ID}]\n  - 📥 {ANSWER}\n")),
        "the answer stays nested beneath its context parent:\n{body}"
    );
}

/// A struck context parent stays terminal without closing a live projected
/// answer leaf. Once the request Todo is already closed, that leaf remains
/// ordinary queue work.
#[test]
fn a_struck_answer_parent_keeps_its_projected_child_live() {
    let dir = tempfile::tempdir().unwrap();
    let rows = format!(
        "- 🙋 {PAUSED_ROW} [#{REQUEST_ID}]\n- separate flat head\n- ~~[#{REQUEST_ID}]~~\n  - 📥 {ANSWER}\n"
    );
    let doc = closeout_session(dir.path(), &rows, &answer_row_todo('x'));
    let path = doc.to_str().unwrap();
    dispatch_and_report(dir.path(), path, PAUSED_ROW, "subagent:stalereplay");

    let state = preflight_json(dir.path(), &doc);
    let prompts = string_list(&state, "queue_prompts");
    let dispatch = string_list(&state, "queue_subagent_dispatch");
    assert!(prompts.iter().any(|item| item == ANSWER), "{prompts:#?}");
    assert!(dispatch.iter().any(|item| item == ANSWER), "{dispatch:#?}");
    let body = run_body(&doc, "run");
    assert!(
        body.contains(&format!("- ~~[#{REQUEST_ID}]~~\n  - 📥 {ANSWER}\n")),
        "the live answer leaf leaves its context parent terminal:\n{body}"
    );
    assert!(
        !body.contains('🙋'),
        "a closed Todo clears the paused row's 🙋:\n{body}"
    );

    let response = format!(
        "<!-- patch:exchange -->\n### Re: stale replay — gpt-5\n\n> **Queue prompt:** 🙋 {PAUSED_ROW} [#{REQUEST_ID}]\n\nClosed.\n<!-- /patch:exchange -->\n"
    );
    let output = run_agent_doc_stdin(dir.path(), &["respond", path, "--force-disk"], &response);
    assert_ok(&output, "respond");
    let body = run_body(&doc, "run");
    assert!(
        body.starts_with(&format!("- ~~{PAUSED_ROW} [#{REQUEST_ID}]~~")),
        "the exact echo strikes the row once its request is closed:\n{body}"
    );
}
