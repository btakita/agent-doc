//! One-line queue tree edits and the `run` component alias
//! (`#patchruncomponent`, agent-doc-bugs.md 2026-11-25).
//!
//! `agent-doc patch FILE run --id main` refused `<!-- agent:run id="main" -->`
//! and no command edited one `%%` note inside a queue tree, so the coordinator
//! could only supersede rows. These drive the real binary on a temp document.
use std::fs;
use std::process::Command;

const CONTROLLER_LAUNCH_REASON: &str =
    "queue row edit CLI fixture drives patch/queue edit/queue claim against a temp document";

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

fn assert_ok(output: &std::process::Output, what: &str) -> String {
    assert!(
        output.status.success(),
        "{what} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn assert_refused(output: &std::process::Output, needle: &str) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success() && stderr.contains(needle),
        "expected a refusal containing {needle:?}\nstderr:\n{stderr}"
    );
}

const STALE: &str =
    "%% GH #435 stays open until it is verified on the Coder host after that release. %%";
const FRESH: &str =
    "%% closed 2026-11-24; tracked in https://github.com/btakita/agent-doc/issues/442 §2 %%";

fn session(dir: &std::path::Path) -> std::path::PathBuf {
    fs::create_dir_all(dir.join(".agent-doc/locks")).unwrap();
    let doc = dir.join("session.md");
    let content = format!(
        concat!(
            "---\nagent_doc_session: queue-row-edit\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: setup — gpt-5 · 2026-11-25T08:00-04:00\n\nReady.\n",
            "<!-- agent:boundary:keep -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Run <!-- agent:run id=\"main\" subagents priority go -->\n",
            "- ### https://github.com/btakita/agent-doc/issues/435 [#q-7174ad2918b2]\n",
            "  - {stale}\n",
            "- do [#alpha]\n",
            "<!-- /agent:run -->\n\n",
            "## Backlog <!-- agent:backlog queue -->\n",
            "- [ ] [#alpha] fix the alpha defect\n",
            "<!-- /agent:backlog -->\n",
        ),
        stale = STALE,
    );
    fs::write(&doc, &content).unwrap();
    git(dir, &["init", "-q"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "user.name", "Test User"]);
    git(dir, &["add", "session.md"]);
    git(dir, &["commit", "-q", "--no-verify", "-m", "initial"]);
    doc
}

#[test]
fn patch_accepts_the_run_component_name() {
    let dir = tempfile::tempdir().unwrap();
    let doc = session(dir.path());
    let path = doc.to_str().unwrap();
    let before = fs::read_to_string(&doc).unwrap();
    let output = run_agent_doc(
        dir.path(),
        &[
            "patch",
            path,
            "run",
            "--id",
            "main",
            "--",
            "- do [#alpha]\n",
        ],
    );
    assert_ok(&output, "patch run --id main");
    let after = fs::read_to_string(&doc).unwrap();
    let start = before.find("- ### https://").unwrap();
    let end = before.find("<!-- /agent:run -->").unwrap();
    assert_eq!(
        after,
        format!("{}- do [#alpha]\n{}", &before[..start], &before[end..]),
        "only the agent:run body is replaced"
    );
}

#[test]
fn queue_edit_replaces_one_note_under_a_row_and_refuses_ambiguity() {
    let dir = tempfile::tempdir().unwrap();
    let doc = session(dir.path());
    let path = doc.to_str().unwrap();
    let before = fs::read_to_string(&doc).unwrap();

    let dry = assert_ok(
        &run_agent_doc(
            dir.path(),
            &[
                "queue",
                "edit",
                path,
                "--row",
                "q-7174ad2918b2",
                "--find",
                STALE,
                "--text",
                FRESH,
                "--dry-run",
            ],
        ),
        "queue edit --dry-run",
    );
    assert!(dry.starts_with("(dry run)"), "{dry}");
    assert_eq!(
        fs::read_to_string(&doc).unwrap(),
        before,
        "dry run writes nothing"
    );

    let out = assert_ok(
        &run_agent_doc(
            dir.path(),
            &[
                "queue",
                "edit",
                path,
                "--id",
                "main",
                "--row",
                "[#q-7174ad2918b2]",
                "--find",
                STALE,
                "--text",
                FRESH,
            ],
        ),
        "queue edit",
    );
    assert!(out.contains("edited line"), "{out}");
    assert_eq!(
        fs::read_to_string(&doc).unwrap(),
        before.replacen(&format!("  - {STALE}\n"), &format!("  - {FRESH}\n"), 1)
    );

    // The note is gone now, and a marker is never accepted as row text.
    assert_refused(
        &run_agent_doc(
            dir.path(),
            &["queue", "edit", path, "--find", STALE, "--text", FRESH],
        ),
        "no line in any queue reads",
    );
    assert_refused(
        &run_agent_doc(
            dir.path(),
            &[
                "queue",
                "edit",
                path,
                "--find",
                FRESH,
                "--text",
                "<!-- /agent:run -->",
            ],
        ),
        "component",
    );
}

/// A row a worker is claimed for keys its claim on its text: changing that
/// text (or deleting the row) refuses, while other queue lines stay editable.
#[test]
fn queue_edit_refuses_to_change_a_claimed_rows_identity() {
    let dir = tempfile::tempdir().unwrap();
    let doc = session(dir.path());
    let path = doc.to_str().unwrap();
    assert_ok(
        &run_agent_doc(
            dir.path(),
            &[
                "queue",
                "claim",
                path,
                "--item",
                "do [#alpha]",
                "--owner",
                "subagent:alpha",
            ],
        ),
        "queue claim",
    );
    let before = fs::read_to_string(&doc).unwrap();
    assert_refused(
        &run_agent_doc(
            dir.path(),
            &[
                "queue",
                "edit",
                path,
                "--row",
                "alpha",
                "--text",
                "do [#beta]",
            ],
        ),
        "subagent:alpha",
    );
    assert_eq!(fs::read_to_string(&doc).unwrap(), before);
    assert_refused(
        &run_agent_doc(
            dir.path(),
            &["queue", "edit", path, "--row", "alpha", "--delete"],
        ),
        "deleting it would detach the claim",
    );
    assert_eq!(fs::read_to_string(&doc).unwrap(), before);

    // A note elsewhere in the queue stays editable while the claim is held.
    assert_ok(
        &run_agent_doc(
            dir.path(),
            &["queue", "edit", path, "--find", STALE, "--text", FRESH],
        ),
        "edit a note while a claim is held",
    );
    assert!(
        fs::read_to_string(&doc)
            .unwrap()
            .contains(&format!("  - {FRESH}\n"))
    );
}
