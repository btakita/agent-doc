//! `#commitoutsidegit`: an explicit commit request for a document outside any
//! git work tree must fail closed and name the missing precondition.
//!
//! `agent-doc commit` used to exit 0 printing `nothing to commit: ... already
//! matches HEAD` although there is no `HEAD`, after creating a snapshot and
//! repositioning the boundary in the working tree. `compact --commit` compacted
//! the document and then blamed a stale-supervisor CRDT replay. `write --commit`
//! and `finalize` already refuse with `... requires a git repository ...`; these
//! tests pin the same refusal for the remaining commit commands, with and
//! without a `.agent-doc/` project ancestor, and pin that an in-repo document
//! reached through a relative path from a non-git cwd still commits.

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
use std::path::Path;
use std::process::Command as ProcessCommand;
use tempfile::TempDir;

/// `#integrationtesttargets`: this target drives the real `agent-doc` CLI, and
/// a controller launch by that CLI is production behavior, so every spawned
/// command opts in with this reason.
const CONTROLLER_LAUNCH_REASON: &str = "commit CLI commands close out through the project controller, which they launch as production does";

/// Document-only content (no `### Re:` response), which `agent-doc commit` may
/// stage as document-only drift inside a repository.
const PROMPT_ONLY_DOC: &str = "---\nagent_doc_session: commitoutsidegit\nagent: claude\nagent_doc_format: template\n---\n\n<!-- agent:exchange -->\nnotes the operator typed\n<!-- /agent:exchange -->\n";

const SESSION_DOC: &str = "---\nagent_doc_session: commitoutsidegit\nagent: claude\nagent_doc_format: template\n---\n\n<!-- agent:exchange -->\n### Re: one\n\nbody one\n\n### Re: two\n\nbody two\n\n### Re: three\n\nbody three\n\n<!-- /agent:exchange -->\n";

fn agent_doc(ceiling: &Path) -> Command {
    let mut command = cargo_bin_cmd!("agent-doc");
    command.env(
        agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
        CONTROLLER_LAUNCH_REASON,
    );
    // `#testisolationtests`: keep the project-root walk inside the fixture so
    // it never adopts `$HOME/.agent-doc`.
    command.env("AGENT_DOC_ROOT_CEILING_DIRECTORIES", ceiling);
    if std::env::var_os(agent_doc_fs::TEST_RUNTIME_SCOPE_ENV).is_none() {
        command.env(agent_doc_fs::TEST_RUNTIME_SCOPE_ENV, std::env::temp_dir());
    }
    command.env_remove("TMUX").env_remove("TMUX_PANE");
    command
}

fn assert_outside_git(tmp: &TempDir) {
    let status = ProcessCommand::new("git")
        .current_dir(tmp.path())
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .unwrap();
    assert!(
        !status.status.success(),
        "fixture {} must not be inside a git work tree",
        tmp.path().display()
    );
}

fn fixture(with_project_dir: bool) -> (TempDir, std::path::PathBuf) {
    let tmp = TempDir::new().unwrap();
    assert_outside_git(&tmp);
    if with_project_dir {
        fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
    }
    let doc = tmp.path().join("session.md");
    fs::write(&doc, SESSION_DOC).unwrap();
    (tmp, doc)
}

fn git(root: &Path, args: &[&str]) {
    let status = ProcessCommand::new("git")
        .current_dir(root)
        .args(args)
        .status()
        .unwrap();
    assert!(
        status.success(),
        "git {args:?} failed in {}",
        root.display()
    );
}

fn assert_commit_refused(tmp: &TempDir, doc: &Path, cwd: &Path, arg: &str) {
    agent_doc(tmp.path())
        .current_dir(cwd)
        .args(["commit", arg])
        .assert()
        .failure()
        .stdout(predicates::str::contains("already matches HEAD").not())
        .stdout(predicates::str::contains("[commit] committed").not())
        .stderr(predicates::str::contains(
            "commit requires a git repository so the document can reach a committed state",
        ))
        .stderr(predicates::str::contains("is not inside a git work tree"))
        .stderr(predicates::str::contains("nothing was committed"))
        .stderr(predicates::str::contains("git init"))
        .stderr(predicates::str::contains(format!("agent-doc commit {arg}")));
    assert_eq!(
        fs::read_to_string(doc).unwrap(),
        SESSION_DOC,
        "the refusal must come before the boundary reposition touches the file"
    );
}

#[test]
fn commit_outside_git_with_project_dir_fails_closed() {
    let (tmp, doc) = fixture(true);
    assert_commit_refused(&tmp, &doc, Path::new("/"), doc.to_str().unwrap());
}

#[test]
fn commit_outside_git_without_project_dir_fails_closed() {
    let (tmp, doc) = fixture(false);
    assert_commit_refused(&tmp, &doc, Path::new("/"), doc.to_str().unwrap());
    assert!(
        !tmp.path().join(".agent-doc").exists(),
        "a refused commit must not create project state"
    );
}

#[test]
fn commit_outside_git_relative_path_fails_closed() {
    let (tmp, doc) = fixture(true);
    assert_commit_refused(&tmp, &doc, tmp.path(), "session.md");
}

/// The relative path is resolved against the cwd before the git check: a
/// document inside a repository subdirectory, named from a non-git cwd, is in a
/// repository and must still commit.
#[test]
fn commit_relative_path_into_repo_from_non_git_cwd_still_commits() {
    let tmp = TempDir::new().unwrap();
    assert_outside_git(&tmp);
    let repo = tmp.path().join("repo");
    fs::create_dir_all(repo.join(".agent-doc")).unwrap();
    fs::write(repo.join(".gitignore"), ".agent-doc/\n").unwrap();
    let doc = repo.join("session.md");
    fs::write(&doc, PROMPT_ONLY_DOC).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test User"]);
    git(&repo, &["add", ".gitignore"]);
    git(&repo, &["commit", "-q", "-m", "initial", "--no-verify"]);

    agent_doc(tmp.path())
        .current_dir(tmp.path())
        .args(["commit", "repo/session.md"])
        .assert()
        .success()
        .stderr(predicates::str::contains("requires a git repository").not())
        .stdout(predicates::str::contains("[commit] committed"));

    let show = ProcessCommand::new("git")
        .current_dir(&repo)
        .args(["show", "HEAD:session.md"])
        .output()
        .unwrap();
    assert!(show.status.success(), "the document must be in HEAD");
}

#[test]
fn compact_commit_outside_git_fails_closed_before_compacting() {
    let (tmp, doc) = fixture(true);
    agent_doc(tmp.path())
        .current_dir(tmp.path())
        .args([
            "compact",
            doc.to_str().unwrap(),
            "--component",
            "exchange",
            "--keep",
            "1",
            "--commit",
            "--force-disk",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "compact --commit requires a git repository so the document can reach a committed state",
        ))
        .stderr(predicates::str::contains("git init"))
        .stderr(predicates::str::contains("stale-supervisor").not());
    assert_eq!(
        fs::read_to_string(&doc).unwrap(),
        SESSION_DOC,
        "compact --commit must refuse before compacting the document"
    );
}
