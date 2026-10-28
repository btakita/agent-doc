//! `agent:todo` stays in sync with the queue rows that name it (`#todosync`).
//!
//! The operator's 2026-10-27 report: "`agent:todo` should always be in sync,
//! like how `agent:backlog` is always in sync." A `do [#id]` row backed by a
//! backlog item shares that item's lifecycle: its dispatch brief quotes the
//! backlog text, and checking the item off closes the row. A row backed by an
//! `agent:todo` item got neither: `queue brief` printed "No backlog/review item
//! in the document carries #pmsyncquestions", and a checked Todo left its
//! `📥 do [#id]` row live and dispatchable.
use std::fs;
use std::process::Command;

const CONTROLLER_LAUNCH_REASON: &str =
    "todo queue sync CLI fixture drives queue brief/preflight against a temp document";

fn git(root: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
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

fn session(dir: &std::path::Path, run_rows: &str, todo_rows: &str) -> std::path::PathBuf {
    fs::create_dir_all(dir.join(".agent-doc/locks")).unwrap();
    let doc = dir.join("session.md");
    let content = format!(
        concat!(
            "---\nagent_doc_session: todo-sync\nagent_doc_format: template\n",
            "prompt_presets:\n  \"#auth\": \"I authorize you to do this.\"\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append subagents -->\n",
            "### Re: dispatch — gpt-5 · 2026-10-27T08:00-04:00\n\nReady.\n",
            "<!-- agent:boundary:keep -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Run <!-- agent:run subagents preset=\"#auth\" priority go -->\n",
            "{run_rows}",
            "<!-- /agent:run -->\n\n",
            "## Todo <!-- agent:todo priority -->\n",
            "{todo_rows}",
            "<!-- /agent:todo -->\n\n",
            "## Backlog <!-- agent:backlog priority queue -->\n",
            "- [ ] [#bk1] backlog work\n",
            "<!-- /agent:backlog -->\n",
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

fn run_body(doc: &std::path::Path) -> String {
    let content = fs::read_to_string(doc).unwrap();
    let start = content.find("<!-- agent:run ").unwrap();
    let body_start = start + content[start..].find('\n').unwrap() + 1;
    let end = content.find("<!-- /agent:run -->").unwrap();
    content[body_start..end].to_string()
}

fn assert_ok(output: &std::process::Output, what: &str) {
    assert!(
        output.status.success(),
        "{what} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The live `#pmsyncquestions` shape: a Todo whose nested questions carry the
/// operator's nested inline answers.
const PM_TODO: &str = concat!(
    "- [ ] [#pmq] Answer the PM-sync design questions\n",
    "  - One component per provider, or one `agent:tracker`?\n",
    "    - One per provider.\n",
    "  - Should tracker refreshes be committed to git?\n",
    "    - Only when there is a change.\n",
);

/// The brief for a Todo-backed `do [#id]` head quotes the Todo and every
/// nested line verbatim, as it quotes a backlog item.
#[test]
fn a_todo_backed_head_brief_quotes_the_todo_with_its_nested_answers() {
    let dir = tempfile::tempdir().unwrap();
    let doc = session(dir.path(), "- do [#pmq]\n", PM_TODO);
    let path = doc.to_str().unwrap();
    let brief = run_agent_doc(
        dir.path(),
        &["queue", "brief", path, "--item", "do [#pmq]", "--json"],
    );
    assert_ok(&brief, "queue brief");
    let brief: serde_json::Value = serde_json::from_slice(&brief.stdout).unwrap();
    assert!(brief.get("unresolved_ids").is_none(), "{brief:#}");
    let tracked = &brief["tracked_items"][0];
    assert_eq!(tracked["id"], "pmq", "{brief:#}");
    assert_eq!(tracked["component"], "todo", "{brief:#}");
    assert_eq!(
        tracked["text"].as_str().unwrap(),
        concat!(
            "Answer the PM-sync design questions\n",
            "  - One component per provider, or one `agent:tracker`?\n",
            "    - One per provider.\n",
            "  - Should tracker refreshes be committed to git?\n",
            "    - Only when there is a change.",
        ),
        "nested sub-items and replies keep their indentation"
    );
    let preamble = brief["subagent_prompt_preamble"].as_str().unwrap();
    assert!(preamble.contains("todo item #pmq (verbatim)"), "{preamble}");
    assert!(
        preamble.contains(">     - Only when there is a change."),
        "{preamble}"
    );
    assert!(
        !preamble.contains("ask the coordinator for its text"),
        "{preamble}"
    );
}

/// Checking a Todo off closes the queue row that names it, as checking a
/// backlog item off does; an open Todo's row stays live, and the Todo text is
/// never rewritten.
#[test]
fn an_operator_checked_todo_closes_its_queue_row() {
    let dir = tempfile::tempdir().unwrap();
    let todo = format!("{PM_TODO}- [ ] [#other] call the vendor\n");
    let doc = session(
        dir.path(),
        "- do [#pmq]\n- do [#bk1]\n- do [#other]\n",
        &todo,
    );
    let path = doc.to_str().unwrap();
    assert_ok(
        &run_agent_doc(dir.path(), &["preflight", path]),
        "preflight",
    );
    assert!(
        run_body(&doc).contains("📥 do [#pmq]\n"),
        "an open Todo's row is live:\n{}",
        run_body(&doc)
    );

    // The operator checks both the Todo and the backlog item.
    let checked = fs::read_to_string(&doc)
        .unwrap()
        .replace("- [ ] [#pmq]", "- [x] [#pmq]")
        .replace("- [ ] [#bk1]", "- [x] [#bk1]");
    fs::write(&doc, &checked).unwrap();
    let output = run_agent_doc(dir.path(), &["preflight", path]);
    assert_ok(&output, "preflight after the operator checks");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("\"do [#pmq]\" source=todo_checked"),
        "{stderr}"
    );

    let body = run_body(&doc);
    assert!(body.contains("- ~~do [#bk1]~~"), "backlog parity:\n{body}");
    assert!(
        body.contains("- ~~do [#pmq]~~"),
        "the checked Todo closes its row:\n{body}"
    );
    assert!(
        body.contains("📥 do [#other]\n"),
        "an open Todo's row stays live:\n{body}"
    );
    let after = fs::read_to_string(&doc).unwrap();
    assert!(
        after.contains(&PM_TODO.replacen("- [ ] [#pmq]", "- [x] [#pmq]", 1)),
        "the Todo and its nested answers are untouched:\n{after}"
    );
    assert!(
        after.contains("- [ ] [#other] call the vendor\n"),
        "{after}"
    );
}

/// A legacy Todo that shares an OPEN backlog item's id is that work's operator
/// prerequisite, not the work: checking it leaves the backlog row live.
#[test]
fn a_checked_todo_sharing_an_open_backlog_id_keeps_the_backlog_row() {
    let dir = tempfile::tempdir().unwrap();
    let doc = session(
        dir.path(),
        "- do [#bk1]\n",
        "- [x] [#bk1] approve the backlog work\n",
    );
    let path = doc.to_str().unwrap();
    let output = run_agent_doc(dir.path(), &["preflight", path]);
    assert_ok(&output, "preflight");
    let body = run_body(&doc);
    assert!(body.contains("📥 do [#bk1]\n"), "{body}");
    assert!(!body.contains("~~"), "{body}");
}
