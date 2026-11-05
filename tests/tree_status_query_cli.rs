//! Queue tree-status query (`#treestatusquery`).
//!
//! The operator asks whether a tree or subtree has actionable or deletable
//! work, and the binary answers from the queue tree without a model turn:
//! `agent-doc queue tree-status FILE --line N` for editors and terminals, and a
//! `status?` child line that preflight replaces with Status reply rows instead
//! of dispatching it.
use std::fs;
use std::process::Command;

const CONTROLLER_LAUNCH_REASON: &str =
    "tree status query CLI fixture drives queue tree-status/preflight against a temp document";

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

fn assert_ok(output: &std::process::Output, what: &str) {
    assert!(
        output.status.success(),
        "{what} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

const QUEUE: &str = concat!(
    "- 🚧 [#subagent:alpha#1] do [#alpha]\n",
    "- ~~do [#gamma]~~\n",
    "  - ~~do [#gamma-child]~~\n",
    "- ⏭️ do [#epsilon]\n",
);

fn session(dir: &std::path::Path, queue_rows: &str) -> std::path::PathBuf {
    fs::create_dir_all(dir.join(".agent-doc/locks")).unwrap();
    let doc = dir.join("session.md");
    let content = format!(
        concat!(
            "---\nagent_doc_session: tree-status\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: setup — gpt-5 · 2026-11-05T08:00-04:00\n\nReady.\n",
            "<!-- agent:boundary:keep -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue <!-- agent:queue id=\"main\" -->\n",
            "{queue_rows}",
            "<!-- /agent:queue -->\n",
        ),
        queue_rows = queue_rows,
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

fn line_of(content: &str, needle: &str) -> usize {
    content
        .lines()
        .position(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("{needle} not in:\n{content}"))
        + 1
}

#[test]
fn tree_status_answers_for_the_subtree_at_a_line() {
    let dir = tempfile::tempdir().unwrap();
    let doc = session(dir.path(), QUEUE);
    let path = doc.to_str().unwrap();
    let content = fs::read_to_string(&doc).unwrap();
    let marker = line_of(&content, "<!-- agent:queue").to_string();

    let output = run_agent_doc(
        dir.path(),
        &["queue", "tree-status", path, "--line", &marker],
    );
    assert_ok(&output, "queue tree-status");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("queue main — 0 you · 1 agent · 1 deletable · 1 need review"),
        "{text}"
    );
    assert!(text.contains("Next: you: none"), "{text}");
    assert!(text.contains("let the running attempt finish"), "{text}");

    let gamma = line_of(&content, "do [#gamma]~~");
    let output = run_agent_doc(
        dir.path(),
        &[
            "queue",
            "tree-status",
            path,
            "--line",
            &gamma.to_string(),
            "--json",
        ],
    );
    assert_ok(&output, "queue tree-status --json");
    let status: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(status["id"], "gamma", "{status:#}");
    assert_eq!(status["deletable"][0]["line"], gamma, "{status:#}");
    assert_eq!(status["deletable"][0]["end_line"], gamma + 1, "{status:#}");
    assert_eq!(fs::read_to_string(&doc).unwrap(), content, "read-only");

    let output = run_agent_doc(dir.path(), &["queue", "tree-status", path, "--line", "1"]);
    assert!(!output.status.success(), "line 1 is outside every queue");
}

/// Preflight on `queue_rows` (whose snapshot is taken without the edit),
/// after `edit` rewrote the document; returns the contract and the document.
fn preflight_after(queue_rows: &str, edit: &dyn Fn(&str) -> String) -> (serde_json::Value, String) {
    let dir = tempfile::tempdir().unwrap();
    let doc = session(dir.path(), queue_rows);
    let path = doc.to_str().unwrap();
    let edited = edit(&fs::read_to_string(&doc).unwrap());
    fs::write(&doc, &edited).unwrap();
    let output = run_agent_doc(dir.path(), &["preflight", path]);
    assert_ok(&output, "preflight");
    let contract: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    (contract, fs::read_to_string(&doc).unwrap())
}

#[test]
fn preflight_answers_a_status_query_without_dispatching_it() {
    // Same document, without and with a whole-queue `status?` line.
    let (baseline, _) = preflight_after(QUEUE, &|content| content.to_string());
    let (contract, content) = preflight_after(QUEUE, &|content| {
        content.replace("<!-- /agent:queue -->", "- status?\n<!-- /agent:queue -->")
    });
    assert!(!content.contains("- status?"), "{content}");
    assert_eq!(content.matches("**Status:**").count(), 1, "{content}");
    // Queue maintenance leaves the Status rows alone: no stable id, no
    // lifecycle projection, no navigation heading.
    assert!(!content.contains("[#q-"), "{content}");
    assert!(!content.contains("📥 **"), "{content}");
    assert!(!content.contains("### **Status:**"), "{content}");
    assert!(
        content.contains(
            "- **Status:** snapshot — 0 you · 1 agent · 1 deletable · 1 need review ▫️\n"
        ),
        "{content}"
    );
    assert!(
        content.contains("  - **Deletable:** “do `#gamma`” — every row is struck"),
        "{content}"
    );
    // The query adds nothing to the turn: the same diff, queue prompts, and
    // change verdict as the document without it, and no Status text reaches
    // the agent.
    for key in [
        "diff",
        "queue_prompts",
        "no_changes",
        "queue_drainable_head_count",
    ] {
        assert_eq!(contract[key], baseline[key], "{key}: {contract:#}");
    }
    let rendered = contract.to_string();
    assert!(!rendered.contains("status?"), "{contract:#}");
    assert!(!rendered.contains("**Status:**"), "{contract:#}");
}

#[test]
fn a_status_query_on_a_settled_queue_opens_no_turn() {
    let settled = concat!(
        "- ~~do [#gamma]~~\n",
        "  - ~~do [#gamma-child]~~\n",
        "- ~~do [#omega]~~\n",
    );
    let (contract, content) = preflight_after(settled, &|content| {
        content.replace(
            "  - ~~do [#gamma-child]~~\n",
            "  - ~~do [#gamma-child]~~\n  - status?\n",
        )
    });
    assert!(!content.contains("- status?"), "{content}");
    assert!(
        content.contains(concat!(
            "  - **Status:** snapshot — 0 you · 0 agent · 1 deletable · 0 need review ▫️\n",
            "    - **Deletable:** “do `#gamma`” — every row is struck and its response records no remaining work ▫️\n",
        )),
        "{content}"
    );
    assert_eq!(contract["no_changes"], true, "{contract:#}");
    assert!(contract["diff"].is_null(), "{contract:#}");
}
