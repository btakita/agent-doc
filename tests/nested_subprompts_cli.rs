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
        dispatch.iter().all(|item| !item.starts_with("alpha epic")),
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
