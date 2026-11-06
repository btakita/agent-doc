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
        run_body(&doc).contains("- ### 📥 do [#pmq]\n"),
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
    assert!(
        body.contains("- ### ~~do [#bk1]~~"),
        "backlog parity:\n{body}"
    );
    assert!(
        body.contains("- ### ~~do [#pmq]~~"),
        "the checked Todo closes its row:\n{body}"
    );
    assert!(
        body.contains("- ### 📥 do [#other]\n"),
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
    assert!(body.contains("- ### 📥 do [#bk1]\n"), "{body}");
    assert!(!body.contains("~~"), "{body}");
}

/// `#answerbyrequestid`: the agent-free path. The operator answered a request
/// copy of a question in a queue row that has been consumed. A plain preflight
/// (no agent command) closes the stale request copy, keeps the decision todo
/// open, and puts the verbatim answer in the cycle contract. A later closeout
/// that re-asks the question is refused with the recorded answer.
#[test]
fn an_answered_question_closes_its_duplicate_and_a_reask_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let doc = session(
        dir.path(),
        "- ~~[#subagent-operator-request-tdport-pick]: Use port 3000.~~\n",
        concat!(
            "- [ ] [#tdport] Choose the demo callback port for local sign-in runs\n",
            "- [ ] [#subagent-operator-request-tdport-pick] Choose the demo callback port for local sign-in runs: 3000 or 47616\n",
        ),
    );
    let path = doc.to_str().unwrap();
    let output = run_agent_doc(dir.path(), &["preflight", path]);
    assert_ok(&output, "preflight");
    let contract: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let answered = contract["answered_decisions"]
        .as_array()
        .expect("answered decisions");
    let fact = answered
        .iter()
        .find(|fact| {
            fact["open_items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item == "tdport")
        })
        .unwrap_or_else(|| panic!("{contract:#}"));
    assert_eq!(fact["answer"], "Use port 3000.", "{fact:#}");
    assert!(
        contract["answered_decisions_guidance"]
            .as_str()
            .unwrap()
            .contains("do not ask again"),
        "{contract:#}"
    );
    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        content.contains("- [x] [#subagent-operator-request-tdport-pick]"),
        "{content}"
    );
    assert!(content.contains("already answered `dec-"), "{content}");
    assert!(content.contains("- [ ] [#tdport]"), "{content}");

    let response = concat!(
        "<!-- patch:exchange -->\n",
        "The port is still open.\n\n",
        "[#subagent-operator-request-tdport-again] subagent operator request\n",
        "- *queue_item:*\n",
        "  - do [#tdport]\n",
        "- *kind:* question\n",
        "- *blocking:* true\n",
        "- *request:*\n",
        "  - Which demo callback port should local sign-in runs use?\n",
        "- *context:*\n",
        "  - The demo needs a fixed port.\n",
        "- *choices:*\n",
        "  - **3000**: use port 3000.\n",
        "  - **47616**: use port 47616.\n",
        "<!-- /agent:exchange -->\n",
    )
    .replace("<!-- /agent:exchange -->", "<!-- /patch:exchange -->");
    let mut child = Command::new(env!("CARGO_BIN_EXE_agent-doc"))
        .env(
            agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
            CONTROLLER_LAUNCH_REASON,
        )
        .env(
            "AGENT_DOC_ROOT_CEILING_DIRECTORIES",
            dir.path().parent().unwrap(),
        )
        .current_dir(dir.path())
        .args(["write", "--commit", path])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    {
        use std::io::Write as _;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(response.as_bytes())
            .unwrap();
    }
    let output = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "re-ask must be refused: {stderr}");
    assert!(
        stderr.contains("closeout refused (#answerbyrequestid)"),
        "{stderr}"
    );
    assert!(stderr.contains("Use port 3000."), "{stderr}");
    assert!(
        !fs::read_to_string(&doc)
            .unwrap()
            .contains("Which demo callback port"),
        "nothing is written before the refusal"
    );
}

/// `#decisionsearchmenu`: `decision search` finds the answered decision by
/// copy id, decision stamp and question wording, reports the verbatim answer
/// and open copies, and says when tsift recall did not run. A plain work
/// turn's contract carries no lookup menu.
#[test]
fn decision_search_finds_answers_and_the_menu_stays_off_work_turns() {
    let dir = tempfile::tempdir().unwrap();
    let doc = session(
        dir.path(),
        "- ~~[#subagent-operator-request-tdport-pick]: Use port 3000.~~\n",
        concat!(
            "- [ ] [#tdport] Choose the demo callback port for local sign-in runs\n",
            "- [ ] [#subagent-operator-request-tdport-pick] Choose the demo callback port for local sign-in runs: 3000 or 47616\n",
        ),
    );
    let path = doc.to_str().unwrap();
    let search = |query: &[&str]| -> serde_json::Value {
        let mut args = vec!["decision", "search", path];
        args.extend_from_slice(query);
        args.push("--json");
        let output = Command::new(env!("CARGO_BIN_EXE_agent-doc"))
            .env("AGENT_DOC_ANSWER_RECALL", "off")
            .env(
                "AGENT_DOC_ROOT_CEILING_DIRECTORIES",
                dir.path().parent().unwrap(),
            )
            .current_dir(dir.path())
            .args(&args)
            .output()
            .unwrap();
        assert_ok(&output, "decision search");
        serde_json::from_slice(&output.stdout).unwrap()
    };
    let by_id = search(&["[#tdport]"]);
    let top = &by_id["hits"][0];
    assert_eq!(top["answer"], "Use port 3000.", "{by_id:#}");
    assert_eq!(top["match"], "copy_id", "{by_id:#}");
    assert!(
        top["location"]
            .as_str()
            .is_some_and(|where_| !where_.is_empty()),
        "{by_id:#}"
    );
    assert!(
        top["open_items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item == "tdport"),
        "{by_id:#}"
    );
    assert_eq!(by_id["recall"]["status"], "unavailable", "{by_id:#}");
    let decision = top["decision_id"].as_str().unwrap().to_string();
    let by_stamp = search(&[&format!("⟦decision:{decision}⟧")]);
    assert_eq!(by_stamp["hits"][0]["match"], "stamp", "{by_stamp:#}");
    let by_words = search(&["callback", "port"]);
    assert_eq!(
        by_words["hits"][0]["decision_id"],
        decision.as_str(),
        "{by_words:#}"
    );
    assert_eq!(by_words["hits"][0]["match"], "wording", "{by_words:#}");
    assert!(
        search(&["kubernetes", "namespace"])["hits"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    // Human-readable output names the recall outcome.
    let output = Command::new(env!("CARGO_BIN_EXE_agent-doc"))
        .env("AGENT_DOC_ANSWER_RECALL", "off")
        .current_dir(dir.path())
        .args(["decision", "search", path, "#tdport"])
        .output()
        .unwrap();
    assert_ok(&output, "decision search (text)");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("answer: “Use port 3000.”"), "{text}");
    assert!(text.contains("tsift recall: unavailable"), "{text}");

    // A work turn on a document with no open copy of an answered question
    // carries no menu.
    let quiet = tempfile::tempdir().unwrap();
    let doc = session(
        quiet.path(),
        "- ~~[#subagent-operator-request-tdport-pick]: Use port 3000.~~\n",
        "- [x] [#subagent-operator-request-tdport-pick] Demo callback port: 3000 or 47616?\n",
    );
    let output = run_agent_doc(
        quiet.path(),
        &["preflight", "--probe", doc.to_str().unwrap()],
    );
    assert_ok(&output, "preflight --probe");
    let contract: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        contract.get("decision_lookup_menu").is_none(),
        "{contract:#}"
    );
}

/// `#decisionsearchnofile`: the first word is FILE only when it names a
/// session document. Without FILE the project's session documents and its
/// `decisions.md` are searched, and each hit names its document.
#[test]
fn decision_search_without_file_searches_the_project() {
    let dir = tempfile::tempdir().unwrap();
    let doc = session(
        dir.path(),
        "- ~~[#subagent-operator-request-tdport-pick]: Use port 3000.~~\n",
        "- [ ] [#subagent-operator-request-tdport-pick] Choose the demo callback port for local sign-in runs: 3000 or 47616\n",
    );
    fs::create_dir_all(dir.path().join("tasks")).unwrap();
    fs::write(
        dir.path().join("tasks/other.md"),
        concat!(
            "---\nagent_doc_session: other\n---\n\n",
            "## Todo <!-- agent:todo -->\n",
            "- [x] [#licpick] Pick the license for the published crates\n",
            "<!-- /agent:todo -->\n",
            "## Run <!-- agent:run -->\n",
            "- ~~[#licpick]: Apache-2.0 for every crate.~~\n",
            "<!-- /agent:run -->\n",
        ),
    )
    .unwrap();
    // Not session documents: never searched, however they read.
    fs::write(dir.path().join("README.md"), "- ~~[#licpick]: GPL.~~\n").unwrap();
    fs::write(
        dir.path().join("decisions.md"),
        concat!(
            "# Decisions\n\nThis file is the authoritative current decision registry.\n\n",
            "<!-- agent:decision id=\"DEC-license-001\" -->\n## DEC-license-001\n",
            "- State: accepted\n- Updated: 2026-11-05T00:00:00Z\n- Evidence: (none)\n- Supersedes: (none)\n",
            "### Decision\nPublish crates under Apache-2.0.\n### Rationale\nPatent grant.\n<!-- /agent:decision -->\n",
        ),
    )
    .unwrap();
    let path = doc.to_str().unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_agent-doc"))
            .env("AGENT_DOC_ANSWER_RECALL", "off")
            .env(
                "AGENT_DOC_ROOT_CEILING_DIRECTORIES",
                dir.path().parent().unwrap(),
            )
            .current_dir(dir.path())
            .arg("decision")
            .arg("search")
            .args(args)
            .output()
            .unwrap()
    };
    let json = |args: &[&str]| -> serde_json::Value {
        let mut args = args.to_vec();
        args.push("--json");
        let output = run(&args);
        assert_ok(&output, "decision search");
        serde_json::from_slice(&output.stdout).unwrap()
    };
    let documents = |report: &serde_json::Value| -> Vec<String> {
        report["hits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|hit| hit["document"].as_str().unwrap().to_string())
            .collect()
    };

    // No FILE: one hit per source, each naming it; the words need no quotes.
    let report = json(&["license", "for", "crates"]);
    let found = documents(&report);
    assert!(
        found.iter().any(|document| document == "tasks/other.md"),
        "{report:#}"
    );
    assert!(
        found.iter().any(|document| document == "decisions.md"),
        "{report:#}"
    );
    assert!(report.get("document").is_none(), "{report:#}");
    assert_eq!(report["project"]["documents"], 2, "{report:#}");
    assert_eq!(
        report["project"]["registry"]["status"], "read",
        "{report:#}"
    );
    assert_eq!(
        json(&["#licpick"])["hits"][0]["answer"],
        "Apache-2.0 for every crate."
    );
    assert_eq!(
        json(&["DEC-license-001"])["hits"][0]["match"],
        "decision_id"
    );

    // The FILE form keeps its reach and its report shape.
    let report = json(&[path, "license", "for", "crates"]);
    assert!(report["hits"].as_array().unwrap().is_empty(), "{report:#}");
    assert_eq!(report["document"], path, "{report:#}");
    assert!(report.get("project").is_none(), "{report:#}");
    let report = json(&["session.md", "#subagent-operator-request-tdport-pick"]);
    assert_eq!(report["hits"][0]["answer"], "Use port 3000.", "{report:#}");

    // A mistyped FILE, or a FILE alone, is refused rather than searched for.
    for args in [
        &["sesion.md", "port"][..],
        &["README.md", "port"],
        &["session.md"],
    ] {
        let output = run(args);
        assert!(!output.status.success(), "{args:?}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("--project"),
            "{output:?}"
        );
    }
    // `--project` makes those words the query.
    let report = json(&["--project", "session.md"]);
    assert!(report["hits"].as_array().unwrap().is_empty(), "{report:#}");
    assert_eq!(report["project"]["documents"], 2, "{report:#}");

    // Human-readable output names every source and the scope.
    let output = run(&["license", "for", "crates"]);
    assert_ok(&output, "decision search (text)");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains(" in tasks/other.md ["), "{text}");
    assert!(
        text.contains("`DEC-license-001` in decisions.md ["),
        "{text}"
    );
    assert!(text.contains("rationale: Patent grant."), "{text}");
    assert!(
        text.contains("searched 2 session documents under "),
        "{text}"
    );
    assert!(text.contains("and decisions.md (1 records)"), "{text}");

    // Outside a project there is nothing to search.
    let outside = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_agent-doc"))
        .env(
            "AGENT_DOC_ROOT_CEILING_DIRECTORIES",
            outside.path().parent().unwrap(),
        )
        .current_dir(outside.path())
        .args(["decision", "search", "port"])
        .output()
        .unwrap();
    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("searches the current project"),
        "{output:?}"
    );
}

/// `#decisionidstamp`: `preflight --probe` carries the same answered facts as
/// a real preflight, from a read-only simulation. It writes no closure and no
/// decision stamp into the document. A real preflight then stamps the open
/// copy with the decision id the facts name.
#[test]
fn a_probe_preflight_reports_answered_decisions_without_writing() {
    let dir = tempfile::tempdir().unwrap();
    let doc = session(
        dir.path(),
        "- ~~[#subagent-operator-request-tdport-pick]: Use port 3000.~~\n",
        concat!(
            "- [ ] [#tdport] Choose the demo callback port for local sign-in runs\n",
            "- [ ] [#subagent-operator-request-tdport-pick] Choose the demo callback port for local sign-in runs: 3000 or 47616\n",
        ),
    );
    let path = doc.to_str().unwrap();
    let before = fs::read_to_string(&doc).unwrap();
    let output = run_agent_doc(dir.path(), &["preflight", "--probe", path]);
    assert_ok(&output, "preflight --probe");
    let contract: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let answered = contract["answered_decisions"]
        .as_array()
        .unwrap_or_else(|| panic!("probe carries answered_decisions: {contract:#}"));
    let fact = answered
        .iter()
        .find(|fact| {
            fact["open_items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item == "tdport")
        })
        .unwrap_or_else(|| panic!("{contract:#}"));
    assert_eq!(fact["answer"], "Use port 3000.", "{fact:#}");
    assert!(
        contract["answered_decisions_guidance"]
            .as_str()
            .is_some_and(|guidance| guidance.contains("do not ask again")),
        "{contract:#}"
    );
    // `#decisionsearchmenu`: an answer is pushed for an open copy, so the
    // probe carries the lookup menu too.
    assert!(
        contract["decision_lookup_menu"]
            .as_str()
            .is_some_and(
                |menu| menu.contains("agent-doc decision search ") && menu.contains("session.md")
            ),
        "{contract:#}"
    );
    assert_eq!(
        fs::read_to_string(&doc).unwrap(),
        before,
        "probe wrote nothing"
    );

    let output = run_agent_doc(dir.path(), &["preflight", path]);
    assert_ok(&output, "preflight");
    let decision = fact["decision_id"].as_str().unwrap();
    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        content.contains(&format!(
            "- [ ] [#tdport] Choose the demo callback port for local sign-in runs ⟦decision:{decision}⟧"
        )),
        "{content}"
    );
}

/// `#decisiongraph`: an operator `change:` reply supersedes a decision. The
/// impact walk reopens the closed item done on the old answer, flags the
/// decision that `depends on` it, and lists the commit made on the old answer
/// for the operator to keep or undo; `why` from that item walks back to the
/// operator's words. Preflight pushes the same impact into the contract.
#[test]
fn a_changed_decision_reports_its_impact_and_why() {
    let dir = tempfile::tempdir().unwrap();
    let doc = session(
        dir.path(),
        concat!(
            "- ### Waiting on you <!-- agent-doc:waiting-on-you -->\n",
            "  - 🙋 [#tdapprover] Name the approver for docs PR #88 <!-- agent-doc:waiting-item -->\n",
            "    - Add Alex + Sam, requested in 3f2a9c1\n",
            "    - change: Keep the approver as is\n",
            "  - 🙋 [#tdnotify] Decide whether to email the approvers about the review <!-- agent-doc:waiting-item -->\n",
            "    - Yes, email them.\n",
            "    - depends on [#tdapprover]\n",
        ),
        concat!(
            "- [ ] [#tdapprover] Name the approver for docs PR #88\n",
            "- [ ] [#tdnotify] Decide whether to email the approvers about the review\n",
            "- [x] [#tdrequestreview] Request review from Alex + Sam on PR #88 (source: [#tdapprover])\n",
        ),
    );
    let path = doc.to_str().unwrap();

    let output = run_agent_doc(
        dir.path(),
        &["decision", "impact", path, "tdapprover", "--json"],
    );
    assert_ok(&output, "decision impact");
    let impact: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(impact["change"], "supersedes", "{impact:#}");
    assert_eq!(impact["change_source"], "operator", "{impact:#}");
    assert_eq!(impact["to_answer"], "Keep the approver as is", "{impact:#}");
    let flagged = impact["flagged"].as_array().unwrap();
    assert!(
        flagged
            .iter()
            .any(|row| row["node"] == "tdrequestreview" && row["action"] == "reopen"),
        "{impact:#}"
    );
    assert!(
        flagged
            .iter()
            .any(|row| row["relation"] == "depends_on" && row["action"] == "flag"),
        "{impact:#}"
    );
    assert!(
        impact["actions_taken"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["node"] == "commit:3f2a9c1" && row["action"] == "keep_or_undo"),
        "{impact:#}"
    );

    let output = run_agent_doc(
        dir.path(),
        &["decision", "why", path, "tdrequestreview", "--json"],
    );
    assert_ok(&output, "decision why");
    let whys: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let why = &whys[0];
    assert_eq!(why["current_answer"], "Keep the approver as is", "{whys:#}");
    let revisions = why["revisions"].as_array().unwrap();
    assert_eq!(revisions.len(), 2, "{whys:#}");
    assert_eq!(revisions[0]["evidence"][0], "commit:3f2a9c1", "{whys:#}");
    assert_eq!(revisions[1]["relation"], "supersedes", "{whys:#}");

    let output = run_agent_doc(dir.path(), &["decision", "graph", path, "--json"]);
    assert_ok(&output, "decision graph");
    let graph: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        graph["edges"]
            .as_array()
            .unwrap()
            .iter()
            .any(|edge| edge["kind"] == "depends_on" && edge["source"] == "operator"),
        "{graph:#}"
    );

    let output = run_agent_doc(dir.path(), &["preflight", path]);
    assert_ok(&output, "preflight");
    let contract: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let fact = contract["answered_decisions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|fact| fact["answer"] == "Keep the approver as is")
        .cloned()
        .unwrap_or_else(|| panic!("{contract:#}"));
    assert_eq!(
        fact["changed_from"], "Add Alex + Sam, requested in 3f2a9c1",
        "{fact:#}"
    );
    assert!(
        fact["change_impact"]
            .as_array()
            .unwrap()
            .iter()
            .any(|line| line
                .as_str()
                .unwrap()
                .starts_with("reopen #tdrequestreview")),
        "{fact:#}"
    );
}

/// `#decisionregistrygraph`: the project `decisions.md` records and their
/// `Supersedes:` links are in the session's decision graph. A record that
/// supersedes another reopens the closed item done under the old record,
/// lists the old record's evidence to keep or undo, and `why` names both
/// directions of the link.
#[test]
fn registry_records_and_supersedes_links_reach_impact_and_why() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("decisions.md"),
        concat!(
            "# Decisions\n\nThis file is the authoritative current decision registry.\n\n",
            "<!-- agent:decision id=\"DEC-store-v1\" -->\n## DEC-store-v1\n- State: superseded\n",
            "- Updated: 2026-10-01T00:00:00Z\n- Evidence: [#r-aaa111bbb222]\n- Supersedes: (none)\n",
            "### Decision\nUse SQLite for the cache\n### Rationale\nOne file.\n<!-- /agent:decision -->\n\n",
            "<!-- agent:decision id=\"DEC-store-v2\" -->\n## DEC-store-v2\n- State: accepted\n",
            "- Updated: 2026-11-01T00:00:00Z\n- Evidence: (none)\n- Supersedes: DEC-store-v1\n",
            "### Decision\nUse Postgres for the cache\n### Rationale\nConcurrent writers.\n<!-- /agent:decision -->\n\n",
        ),
    )
    .unwrap();
    let doc = session(
        dir.path(),
        "",
        "- [x] [#tdbackfill] Backfill the cache under [DEC-store-v1](decision:DEC-store-v1)\n",
    );
    let path = doc.to_str().unwrap();

    let output = run_agent_doc(
        dir.path(),
        &["decision", "impact", path, "DEC-store-v2", "--json"],
    );
    assert_ok(&output, "decision impact");
    let impact: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(impact["change"], "supersedes", "{impact:#}");
    assert_eq!(
        impact["from_answer"], "Use SQLite for the cache",
        "{impact:#}"
    );
    assert_eq!(
        impact["superseded_decisions"][0], "DEC-store-v1",
        "{impact:#}"
    );
    assert!(
        impact["flagged"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["node"] == "tdbackfill" && row["action"] == "reopen"),
        "{impact:#}"
    );
    assert!(
        impact["actions_taken"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["node"] == "r-aaa111bbb222" && row["action"] == "keep_or_undo"),
        "{impact:#}"
    );

    let output = run_agent_doc(
        dir.path(),
        &["decision", "why", path, "tdbackfill", "--json"],
    );
    assert_ok(&output, "decision why");
    let whys: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(whys[0]["decision_id"], "DEC-store-v1", "{whys:#}");
    assert_eq!(whys[0]["superseded_by"][0], "DEC-store-v2", "{whys:#}");

    let output = run_agent_doc(dir.path(), &["decision", "why", path, "DEC-store-v2"]);
    assert_ok(&output, "decision why text");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("supersedes DEC-store-v1: “Use SQLite for the cache”"),
        "{text}"
    );
}

/// `#decisionstaleaudit`: the agent-free path for a stale todo. The operator's
/// answer to the todo's own id was consumed, and nothing still works on it. A
/// plain preflight closes the todo citing the decision, and a todo whose
/// waiting tree still has an operator row stays open, reported as a typed
/// `stale_tracked_item` signal that `decision validate --document` repeats
/// without failing.
#[test]
fn a_delivered_answer_closes_its_idle_todo_and_reports_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let doc = session(
        dir.path(),
        concat!(
            "- ### 📥 Waiting on you — answer by adding a child line under an item [#q-4775b8697412]\n",
            "  - 🙋 ⟦you: 1 · agent: 0 · done: 0⟧ [#tdalpha] Record the public-Alpha decision and the release owner. <!-- agent-doc:waiting-item -->\n",
            "    - **Next:** you — reply under this item ▫️\n",
            "- ~~[#tdapprovers]: Keep the approver as is.~~\n",
            "- ~~[#tdalpha]: build the preview release~~\n",
        ),
        concat!(
            "- [ ] [#tdapprovers] Name the approver for docs PR #88 and request their review\n",
            "- [ ] [#tdalpha] Record the public-Alpha decision and the release owner.\n",
        ),
    );
    let path = doc.to_str().unwrap();
    let output = run_agent_doc(dir.path(), &["preflight", path]);
    assert_ok(&output, "preflight");
    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("- [x] [#tdapprovers]"), "{content}");
    assert!(content.contains("already answered `dec-"), "{content}");
    assert!(content.contains("- [ ] [#tdalpha]"), "{content}");
    let contract: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let signals = contract["todo_signals"].as_array().expect("todo signals");
    let stale = signals
        .iter()
        .find(|signal| signal["kind"] == "stale_tracked_item")
        .unwrap_or_else(|| panic!("{contract:#}"));
    assert_eq!(stale["todo"], "tdalpha", "{stale:#}");
    assert_eq!(stale["component"], "todo", "{stale:#}");
    assert_eq!(stale["stale_kind"], "answered_but_open", "{stale:#}");
    let decision = stale["decision"].as_str().unwrap().to_string();
    assert!(
        stale["action"]
            .as_str()
            .unwrap()
            .contains("--todo-done tdalpha"),
        "{stale:#}"
    );

    let output = run_agent_doc(
        dir.path(),
        &["decision", "validate", "--json", "--document", path],
    );
    assert_ok(&output, "decision validate");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["valid"], true, "{report:#}");
    let reported = report["stale_tracked_items"].as_array().unwrap();
    assert_eq!(reported.len(), 1, "{report:#}");
    assert_eq!(reported[0]["decision"], decision.as_str(), "{report:#}");
}

/// `#firstprincipleswhy`: `decision why` reconciles the ledger's answer with
/// the first-principles derivation chain. Principles come from all three
/// layers (the global file under `$XDG_CONFIG_HOME`, the project's
/// `.agent-doc/project.md`, the session's own `agent:rules`); an operator
/// answer citing `[#id]`, or a `depends on [#id]` link, makes an
/// operator-confirmed step, and a decision that depends on another walks
/// through that decision's step down to its principle.
#[test]
fn decision_why_walks_the_first_principles_chain() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config");
    fs::create_dir_all(config.join("agent-doc")).unwrap();
    fs::write(
        config.join("agent-doc/principles.md"),
        "<!-- agent:rules -->\n- principle [#open-by-default]: Tools are public unless they hold secrets.\n<!-- /agent:rules -->\n",
    )
    .unwrap();
    let root = dir.path().join("repo");
    fs::create_dir_all(root.join(".agent-doc/locks")).unwrap();
    fs::write(
        root.join(".agent-doc/project.md"),
        "<!-- agent:rules -->\n- principle [#least-surprise]: Pick the default a new user expects.\n<!-- /agent:rules -->\n",
    )
    .unwrap();
    let doc = root.join("session.md");
    fs::write(
        &doc,
        concat!(
            "---\nagent_doc_session: why-chain\nagent_doc_format: template\n---\n\n",
            "## Rules <!-- agent:rules -->\n",
            "- principle [#no-tokens]: Install scripts never embed credentials.\n",
            "<!-- /agent:rules -->\n\n",
            "## Run <!-- agent:run -->\n",
            "- ### Waiting on you <!-- agent-doc:waiting-on-you -->\n",
            "  - 🙋 [#tdpublic] Decide whether haiven-cli is public and open source <!-- agent-doc:waiting-item -->\n",
            "    - Public and open source, per [#open-by-default].\n",
            "  - 🙋 [#tdtoken] Decide whether install.sh embeds a download token <!-- agent-doc:waiting-item -->\n",
            "    - No token in install.sh, per [#no-tokens].\n",
            "    - depends on [#tdpublic]\n",
            "  - 🙋 [#tdport] Choose the demo callback port <!-- agent-doc:waiting-item -->\n",
            "    - Use port 8080.\n",
            "    - depends on [#least-surprise]\n",
            "<!-- /agent:run -->\n\n",
            "## Todo <!-- agent:todo -->\n",
            "- [ ] [#tdpublic] Decide whether haiven-cli is public and open source\n",
            "- [ ] [#tdtoken] Decide whether install.sh embeds a download token\n",
            "- [ ] [#tdport] Choose the demo callback port\n",
            "<!-- /agent:todo -->\n",
        ),
    )
    .unwrap();
    git(&root, &["init", "-q"]);
    let path = doc.to_str().unwrap();
    let why = |args: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_agent-doc"))
            .env(
                agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
                CONTROLLER_LAUNCH_REASON,
            )
            .env("AGENT_DOC_ROOT_CEILING_DIRECTORIES", dir.path())
            .env("XDG_CONFIG_HOME", &config)
            .current_dir(&root)
            .args(args)
            .output()
            .unwrap();
        assert_ok(&output, "decision why");
        output
    };

    let output = why(&["decision", "why", path, "tdtoken", "--json"]);
    let whys: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let token = &whys[0];
    assert_eq!(
        token["current_answer"], "No token in install.sh, per [#no-tokens].",
        "{whys:#}"
    );
    assert_eq!(token["derivation_supported"], true, "{whys:#}");
    let step = &token["derived_from"][0];
    assert_eq!(step["status"], "operator_confirmed", "{whys:#}");
    let premises = step["premises"].as_array().unwrap();
    assert_eq!(premises[0]["node"]["kind"], "principle", "{whys:#}");
    assert_eq!(premises[0]["node"]["id"], "no-tokens", "{whys:#}");
    assert_eq!(premises[1]["node"]["kind"], "decision", "{whys:#}");
    let public_premise = &premises[1]["because"]["steps"][0]["premises"][0]["node"];
    assert_eq!(public_premise["kind"], "principle", "{whys:#}");
    assert_eq!(public_premise["id"], "open-by-default", "{whys:#}");
    let scopes: Vec<(String, String)> = token["principles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|principle| {
            (
                principle["id"].as_str().unwrap().to_string(),
                principle["scope"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        scopes,
        vec![
            ("no-tokens".to_string(), "document".to_string()),
            ("open-by-default".to_string(), "global".to_string()),
        ],
        "{whys:#}"
    );
    // The ledger half is unchanged.
    assert_eq!(token["revisions"][0]["relation"], "initial", "{whys:#}");
    assert_eq!(token["depends_on"][0]["kind"], "decision", "{whys:#}");

    let output = why(&["decision", "why", path, "tdport"]);
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.contains("depends on principle least-surprise “Pick the default a new user expects.”"),
        "{text}"
    );
    assert!(text.contains("  derived from:"), "{text}");
    assert!(text.contains("(project, "), "{text}");
}

/// `#decisionstaleevidence`: an open todo that cites an accepted project
/// `decisions.md` record recording its deliverable done is reported done by
/// evidence, citing the record, by both `decision validate --document` and
/// `decision answered --json`.
#[test]
fn a_cited_registry_record_reports_its_todo_done_by_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let doc = session(
        dir.path(),
        "",
        "- [ ] [#tdlru] Merge PR #77 per DEC-cache-001.\n",
    );
    fs::write(
        dir.path().join("decisions.md"),
        concat!(
            "# Decisions\n\nThis file is the authoritative current decision registry.\n\n",
            "<!-- agent:decision id=\"DEC-cache-001\" -->\n## DEC-cache-001\n",
            "- State: accepted\n- Updated: 2026-11-05T00:00:00Z\n- Evidence: (none)\n- Supersedes: (none)\n",
            "### Decision\nUse the LRU cache; PR #77 merged.\n### Rationale\nHit rate.\n<!-- /agent:decision -->\n\n",
        ),
    )
    .unwrap();
    let path = doc.to_str().unwrap();

    let output = run_agent_doc(
        dir.path(),
        &["decision", "validate", "--json", "--document", path],
    );
    assert_ok(&output, "decision validate");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["valid"], true, "{report:#}");
    let reported = report["stale_tracked_items"].as_array().unwrap();
    assert_eq!(reported.len(), 1, "{report:#}");
    assert_eq!(reported[0]["todo"], "tdlru", "{report:#}");
    assert_eq!(reported[0]["stale_kind"], "done_by_evidence", "{report:#}");
    assert_eq!(reported[0]["decision"], "DEC-cache-001", "{report:#}");

    let output = run_agent_doc(dir.path(), &["decision", "answered", "--json", path]);
    assert_ok(&output, "decision answered");
    let ledger: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let stale = ledger["stale_tracked_items"].as_array().unwrap();
    assert_eq!(stale.len(), 1, "{ledger:#}");
    assert_eq!(stale[0]["link"], "registry", "{ledger:#}");
    assert_eq!(
        stale[0]["location"], "decisions.md DEC-cache-001",
        "{ledger:#}"
    );
}
