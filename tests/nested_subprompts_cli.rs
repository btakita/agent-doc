//! Nested sub-prompts in list components (`#nestedsubprompts`).
//!
//! The operator's 2026-10-27 report: a parent row in `agent:run` with four
//! nested rows was invisible to dispatch (`queue claim` on the parent failed
//! with "no live queue head matches") while each nested row was dispatched as
//! an independent head and stamped with its own lifecycle marker. A nested
//! list under a row is that row's sub-prompts: one head, claimed and
//! dispatched as one unit, with the children carried verbatim in its brief.

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
fn a_parent_with_nested_rows_is_one_head_in_every_runnable_list_spelling() {
    for kind in ["run", "queue", "loop"] {
        let dir = tempfile::tempdir().unwrap();
        let doc = committed_session(dir.path(), kind, &live_rows());
        let state = preflight_json(dir.path(), &doc);

        let prompts = string_list(&state, "queue_prompts");
        let parents: Vec<&String> = prompts.iter().filter(|p| p.starts_with(PARENT)).collect();
        assert_eq!(parents.len(), 1, "{kind}: one parent head: {prompts:#?}");
        for child in CHILDREN {
            assert!(
                parents[0].contains(child),
                "{kind}: the head carries child {child:?}: {prompts:#?}"
            );
            assert!(
                prompts
                    .iter()
                    .filter(|p| p.contains(child))
                    .all(|p| p.starts_with(PARENT)),
                "{kind}: child {child:?} must never be its own head: {prompts:#?}"
            );
        }

        let dispatch = string_list(&state, "queue_subagent_dispatch");
        // `agent:loop` is a recurring pass, not a subagent fan-out: it lists no
        // dispatch work at all. Its heads are still one parent per item.
        let fans_out = kind != "loop";
        assert_eq!(
            dispatch
                .iter()
                .filter(|item| item.starts_with(PARENT))
                .count(),
            usize::from(fans_out),
            "{kind}: the parent is dispatchable exactly once: {dispatch:#?}"
        );
        for child in CHILDREN {
            assert!(
                dispatch
                    .iter()
                    .filter(|item| item.contains(child))
                    .all(|item| item.starts_with(PARENT)),
                "{kind}: child {child:?} is never dispatched on its own: {dispatch:#?}"
            );
        }
        assert!(
            !fans_out || dispatch.iter().any(|item| item == "separate flat head"),
            "{kind}: the flat sibling stays its own head: {dispatch:#?}"
        );
        assert!(
            prompts.iter().any(|item| item == "separate flat head"),
            "{kind}: the flat sibling stays its own head: {prompts:#?}"
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
fn the_parent_is_claimable_by_its_line_and_its_brief_carries_the_children_verbatim() {
    let dir = tempfile::tempdir().unwrap();
    let doc = committed_session(dir.path(), "run", &live_rows());
    let path = doc.to_str().unwrap();

    let brief = run_agent_doc(dir.path(), &["queue", "brief", path, "--item", PARENT]);
    assert_ok(&brief, "queue brief");
    let brief = String::from_utf8_lossy(&brief.stdout).to_string();
    assert!(brief.contains(&format!("> {PARENT}\n")), "{brief}");
    for child in CHILDREN {
        assert!(
            brief.contains(child),
            "brief must carry {child:?}:\n{brief}"
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
                PARENT,
                "--owner",
                "subagent:pmsync",
            ],
        ),
        "queue claim on the parent line",
    );
    let body = run_body(&doc, "run");
    assert!(
        body.starts_with(&format!(
            "- 🟡 [#subagent:pmsync#1] {PARENT}\n  - {}\n  - {}\n",
            CHILDREN[0], CHILDREN[1]
        )),
        "the parent carries the one lifecycle projection and the legacy child marker is retired:\n{body}"
    );
    assert_eq!(
        body.matches("[#subagent:").count(),
        1,
        "children never carry a projection of their own:\n{body}"
    );

    // The operator steers: edits one child and adds another.
    let content = fs::read_to_string(&doc).unwrap();
    let steered = content
        .replace(CHILDREN[1], "This is the second subtask, now steered.")
        .replace(
            &format!("  - {}\n", CHILDREN[3]),
            &format!("  - {}\n  - A brand new fifth subtask.\n", CHILDREN[3]),
        );
    assert_ne!(steered, content);
    fs::write(&doc, &steered).unwrap();

    let state = preflight_json(dir.path(), &doc);
    let dispatch = string_list(&state, "queue_subagent_dispatch");
    assert!(
        dispatch.iter().all(|item| !item.starts_with(PARENT)
            && !item.contains("now steered")
            && !item.contains("fifth subtask")),
        "a claimed parent and its new or edited children are never new dispatch work: {dispatch:#?}"
    );
    assert!(
        dispatch.iter().any(|item| item == "separate flat head"),
        "{dispatch:#?}"
    );

    let claims = run_agent_doc(dir.path(), &["queue", "claims", path]);
    assert_ok(&claims, "queue claims");
    let claims: serde_json::Value = serde_json::from_slice(&claims.stdout).unwrap();
    let claims = claims.as_array().unwrap();
    assert_eq!(claims.len(), 1, "{claims:#?}");
    assert_eq!(claims[0]["owner"], "subagent:pmsync");
    assert_eq!(
        claims[0]["identity"]["key"],
        PARENT.to_ascii_lowercase(),
        "the claim identity is the parent line, stable across child edits"
    );

    // The steered brief is the live parent with its current children.
    let brief = run_agent_doc(
        dir.path(),
        &["queue", "brief", path, "--item", PARENT, "--json"],
    );
    assert_ok(&brief, "queue brief after steering");
    let brief: serde_json::Value = serde_json::from_slice(&brief.stdout).unwrap();
    let item = brief["item"].as_str().unwrap();
    assert!(item.starts_with(PARENT), "{item}");
    assert!(item.contains("now steered"), "{item}");
    assert!(item.contains("A brand new fifth subtask."), "{item}");
}

#[test]
fn a_struck_parent_retires_its_children_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let rows = format!(
        "- ~~{PARENT}~~\n  - {}\n  - {}\n- separate flat head\n",
        CHILDREN[0], CHILDREN[1]
    );
    let doc = committed_session(dir.path(), "run", &rows);
    let state = preflight_json(dir.path(), &doc);
    let prompts = string_list(&state, "queue_prompts");
    let dispatch = string_list(&state, "queue_subagent_dispatch");
    for child in [CHILDREN[0], CHILDREN[1], PARENT] {
        assert!(
            prompts.iter().all(|p| !p.contains(child)),
            "{child:?} is completed work: {prompts:#?}"
        );
        assert!(
            dispatch.iter().all(|p| !p.contains(child)),
            "{child:?} is completed work: {dispatch:#?}"
        );
    }
    assert!(
        dispatch.iter().any(|item| item == "separate flat head"),
        "{dispatch:#?}"
    );
}

/// `#struckparentkids` (sdk-demo.md, 2026-10-27): the operator wrote a new
/// row under a parent the agent had already answered and struck. Before, the
/// struck block swallowed it as completed work: no queue head, no dispatch,
/// no idle wake. It is new work for that parent, so preflight lists the
/// reopened block (verbatim, with the struck parent and answered child as
/// context) as one dispatchable head, and the parent line claims it.
#[test]
fn a_row_added_under_a_struck_parent_is_dispatched_as_new_work() {
    let dir = tempfile::tempdir().unwrap();
    let parent = "#subagent-operator-request-handoff-integration-id";
    let answered = "Describe the choices in more detail. Why do we need a SBX/STG id?";
    let added = "I don't know if you understand that we moved onto project templates.";
    let closed = format!("- ~~{parent}~~\n  - ~~{answered}~~\n- separate flat head\n");
    let doc = committed_session(dir.path(), "run", &closed);
    let path = doc.to_str().unwrap();

    let state = preflight_json(dir.path(), &doc);
    let dispatch = string_list(&state, "queue_subagent_dispatch");
    assert!(
        dispatch.iter().all(|item| !item.contains(parent)),
        "a closed block is never work: {dispatch:#?}"
    );

    let content = fs::read_to_string(&doc).unwrap();
    let reopened_rows = format!("- ~~{parent}~~\n  - ~~{answered}~~\n  - {added}\n");
    fs::write(
        &doc,
        content.replace(
            &format!("- ~~{parent}~~\n  - ~~{answered}~~\n"),
            &reopened_rows,
        ),
    )
    .unwrap();

    let head = format!("~~{parent}~~\n  - ~~{answered}~~\n  - {added}");
    let state = preflight_json(dir.path(), &doc);
    let prompts = string_list(&state, "queue_prompts");
    assert!(
        prompts.iter().any(|item| item == &head),
        "the new row is live work with its parent: {prompts:#?}"
    );
    let dispatch = string_list(&state, "queue_subagent_dispatch");
    assert!(
        dispatch.iter().any(|item| item == &head),
        "the reopened block is dispatchable: {dispatch:#?}"
    );

    let handle = format!("~~{parent}~~");
    let brief = run_agent_doc(
        dir.path(),
        &["queue", "brief", path, "--item", &handle, "--json"],
    );
    assert_ok(&brief, "queue brief on the reopened parent line");
    let brief: serde_json::Value = serde_json::from_slice(&brief.stdout).unwrap();
    let item = brief["item"].as_str().unwrap();
    assert!(item.contains(added), "{item}");
    assert!(
        item.contains(answered),
        "the answered child is context: {item}"
    );
    assert_ok(
        &run_agent_doc(
            dir.path(),
            &[
                "queue",
                "claim",
                path,
                "--item",
                &handle,
                "--owner",
                "subagent:reopen",
            ],
        ),
        "queue claim on the reopened parent line",
    );
    assert!(
        run_body(&doc, "run").contains(&format!("[#subagent:reopen#1] ~~{parent}~~\n")),
        "{}",
        run_body(&doc, "run")
    );
}

/// `#struckparentkids`: a whole-block strike that left the answered child
/// unstruck (a 0.35.558 strike, as sdk-demo.md shows) stays closed, and
/// preflight makes the document say so by striking the child.
#[test]
fn a_struck_parent_over_an_unstruck_child_is_made_to_read_closed() {
    let dir = tempfile::tempdir().unwrap();
    let rows = format!(
        "- ~~{PARENT}~~\n  - {}\n- separate flat head\n",
        CHILDREN[0]
    );
    let doc = committed_session(dir.path(), "run", &rows);
    let state = preflight_json(dir.path(), &doc);
    let dispatch = string_list(&state, "queue_subagent_dispatch");
    assert!(
        dispatch.iter().all(|item| !item.contains(CHILDREN[0])),
        "{dispatch:#?}"
    );
    assert!(
        run_body(&doc, "run").starts_with(&format!("- ~~{PARENT}~~\n  - ~~{}~~\n", CHILDREN[0])),
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

/// `#nestedechostrike` (live, agent-doc-bugs.md 2026-10-27): the coordinator
/// closed the reported pmsync row with the verbatim nested echo
/// (`> **Queue prompt:** <parent>` plus one `>   - <child>` line per child).
/// Each quoted line was compared with the whole multi-line head, so nothing
/// struck and the row stayed ☑️ with its reported claim.
#[test]
fn a_verbatim_nested_echo_closes_a_reported_parent_and_ends_its_claim() {
    let dir = tempfile::tempdir().unwrap();
    let rows = format!(
        "- {PARENT}\n  - {}\n  - {}\n  - {}\n  - {}\n- separate flat head\n",
        CHILDREN[0], CHILDREN[1], CHILDREN[2], CHILDREN[3]
    );
    let doc = closeout_session(dir.path(), &rows, "- [ ] call the vendor\n");
    let path = doc.to_str().unwrap();
    dispatch_and_report(dir.path(), path, PARENT, "subagent:pmsync");
    assert!(
        run_body(&doc, "run").starts_with(&format!("- ☑️ [#subagent:pmsync#1] {PARENT}\n")),
        "{}",
        run_body(&doc, "run")
    );
    assert_eq!(claim_owners(dir.path(), path), vec!["subagent:pmsync"]);

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
    assert!(
        body.starts_with(&format!("- ~~{PARENT}~~")),
        "the nested echo strikes the parent:\n{body}"
    );
    assert!(!body.contains("☑️"), "{body}");
    assert!(body.contains("separate flat head\n"), "{body}");
    assert!(
        claim_owners(dir.path(), path).is_empty(),
        "the reported attempt ended with the strike"
    );
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

/// `#queueoperatoranswer`: the operator answered a request by writing the request id as
/// its own run row with the answer nested beneath it. The binary read the
/// bare `[#id]` row and its nested answer as two heads, offered the answer as
/// fresh dispatch work, and the first marker it stamped on the bare row made
/// that row own its child: one row fewer, so every marker projection over the
/// queue (claim, `report-complete`, `release`, preflight maintenance) failed
/// with "changed item count". The answer row is one row keyed by the request
/// id, is never dispatch work, and closes its request like
/// `[#request-id]: <answer>`, after which the paused row's exact echo strikes
/// it.
#[test]
fn an_answer_row_nested_under_its_request_id_answers_the_request() {
    let dir = tempfile::tempdir().unwrap();
    let rows = format!(
        "- 🙋 {PAUSED_ROW} [#{REQUEST_ID}]\n- separate flat head\n- [#{REQUEST_ID}]\n  - {ANSWER}\n"
    );
    let doc = closeout_session(dir.path(), &rows, &answer_row_todo(' '));
    let path = doc.to_str().unwrap();

    let state = preflight_json(dir.path(), &doc);
    let prompts = string_list(&state, "queue_prompts");
    let dispatch = string_list(&state, "queue_subagent_dispatch");
    for (what, list) in [("queue_prompts", &prompts), ("dispatch", &dispatch)] {
        assert!(
            list.iter().all(|item| !item.starts_with(ANSWER)),
            "{what}: the nested answer is never its own head: {list:#?}"
        );
    }
    assert!(
        dispatch.iter().all(|item| !item.contains(ANSWER)),
        "the answer row is never fresh dispatch work: {dispatch:#?}"
    );
    assert!(
        dispatch.iter().any(|item| item == "separate flat head"),
        "{dispatch:#?}"
    );
    let body = run_body(&doc, "run");
    assert!(
        body.contains(&format!("[#{REQUEST_ID}]\n  - {ANSWER}\n")),
        "the answer row and its nested answer stay one row, unmarked below its parent:\n{body}"
    );
    assert!(!body.contains("📥 I want"), "{body}");

    // The worker that owns the request reports back; every marker projection
    // over the queue now succeeds.
    dispatch_and_report(dir.path(), path, PAUSED_ROW, "subagent:stalereplay");
    let body = run_body(&doc, "run");
    assert!(
        body.starts_with(&format!(
            "- 🙋☑️ [#subagent:stalereplay#1] {PAUSED_ROW} [#{REQUEST_ID}]\n"
        )),
        "{body}"
    );

    // The closeout echoes the paused row exactly as the Stop hook suggests.
    // The nested answer completes the request's Todo in the same write, which
    // clears 🙋, so the echo strikes the row and ends its reported claim.
    let response = format!(
        "<!-- patch:exchange -->\n### Re: stale replay — gpt-5\n\n> **Queue prompt:** 🙋 {PAUSED_ROW} [#{REQUEST_ID}]\n\nFixed; sync re-applies the latest state.\n<!-- /patch:exchange -->\n"
    );
    let output = run_agent_doc_stdin(dir.path(), &["respond", path, "--force-disk"], &response);
    assert_ok(&output, "respond");

    let after = fs::read_to_string(&doc).unwrap();
    assert!(
        after.contains(&answer_row_todo('x')),
        "the nested answer completes the request's Todo:\n{after}"
    );
    let body = run_body(&doc, "run");
    assert!(
        body.starts_with(&format!("- ~~{PAUSED_ROW} [#{REQUEST_ID}]~~")),
        "the exact echo strikes the answered row:\n{body}"
    );
    assert!(
        body.contains(&format!("- ~~[#{REQUEST_ID}]~~\n  - ~~{ANSWER}~~\n")),
        "the applied answer row closes with its request:\n{body}"
    );
    assert!(!body.contains('🙋'), "{body}");
    assert!(
        claim_owners(dir.path(), path).is_empty(),
        "the reported attempt ended with the strike"
    );
}

/// `#queueoperatoranswer`, the state the live document was left in: the request's Todo
/// is closed but the paused row still shows `🙋`, the answer row was struck
/// alone and its nested answer was projected `📥` as if it were a new head.
/// Preflight clears the stale `🙋`, closes the nested answer with its struck
/// parent, never dispatches it, and the exact echo then strikes the row.
#[test]
fn a_struck_answer_row_with_a_projected_child_is_closed_work() {
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
    for (what, list) in [("queue_prompts", &prompts), ("dispatch", &dispatch)] {
        assert!(
            list.iter().all(|item| !item.contains(ANSWER)),
            "{what}: the struck answer is closed work: {list:#?}"
        );
    }
    let body = run_body(&doc, "run");
    assert!(
        body.contains(&format!("- ~~[#{REQUEST_ID}]~~\n  - ~~{ANSWER}~~\n")),
        "the struck answer row closes its nested answer too:\n{body}"
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
