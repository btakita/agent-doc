//! An exported `GIT_DIR`/`GIT_WORK_TREE` override that points a submodule's
//! work tree at an unrelated repository must not be treated as a submodule.
//!
//! `git rev-parse --show-superproject-working-tree` still reports the enclosing
//! checkout (its index holds a gitlink at that path), but every git command run
//! "in the superproject" inherits the override, so `HEAD:<submodule>` resolves
//! against the overridden repository and strict closeout failed forever with
//! `parent HEAD:<path>=<missing>`.
//!
//! This lives in its own test binary because it mutates process-wide git
//! environment variables.

use std::fs;
use std::path::Path;
use std::process::Command;

fn git(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed in {}: {}",
        cwd.display(),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn init_repo(repo: &Path) {
    git(repo, &["init"]);
    git(repo, &["config", "user.email", "test@test.com"]);
    git(repo, &["config", "user.name", "Test"]);
}

fn commit_file(repo: &Path, rel: &str, content: &str, msg: &str) {
    let path = repo.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&path, content).unwrap();
    git(repo, &["add", "--", rel]);
    git(repo, &["commit", "-m", msg, "--no-verify"]);
}

#[test]
fn git_dir_override_inside_submodule_worktree_is_not_a_submodule() {
    let outer_dir = tempfile::TempDir::new().unwrap();
    let outer = outer_dir.path().canonicalize().unwrap();
    init_repo(&outer);
    commit_file(&outer, "README.md", "# outer\n", "init outer");

    let origin_dir = tempfile::TempDir::new().unwrap();
    let origin = origin_dir.path();
    init_repo(origin);
    commit_file(origin, "README.md", "# sub\n", "init sub");
    let url = format!("file://{}", origin.display());
    git(
        &outer,
        &["-c", "protocol.file.allow=always", "submodule", "add", &url, "src/sub"],
    );
    git(&outer, &["commit", "-m", "add submodule", "--no-verify"]);
    let sub = outer.join("src/sub");

    // Normal submodule shape: drift detection sees the parent repository.
    let doc = sub.join("tmp/session.md");
    fs::create_dir_all(doc.parent().unwrap()).unwrap();
    fs::write(&doc, "body\n").unwrap();
    assert_eq!(
        agent_doc_git_io::dirs::git_superproject_at(doc.parent().unwrap()),
        Some(outer.clone())
    );

    // Isolated repository rooted at the submodule work tree via GIT_DIR.
    let isolated_git_dir = sub.join("tmp/.isolated-git-dir");
    let output = Command::new("git")
        .args(["init", "--bare"])
        .arg(&isolated_git_dir)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()
        .unwrap();
    assert!(output.status.success());
    // SAFETY: this test binary contains a single test; no other thread reads
    // the environment concurrently.
    unsafe {
        std::env::set_var("GIT_DIR", &isolated_git_dir);
        std::env::set_var("GIT_WORK_TREE", &sub);
        std::env::set_var("GIT_AUTHOR_NAME", "Test");
        std::env::set_var("GIT_AUTHOR_EMAIL", "test@test.com");
        std::env::set_var("GIT_COMMITTER_NAME", "Test");
        std::env::set_var("GIT_COMMITTER_EMAIL", "test@test.com");
    }
    let run = |args: &[&str]| {
        let output = Command::new("git")
            .current_dir(&sub)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    run(&["config", "core.bare", "false"]);
    run(&["add", "--", "tmp/session.md"]);
    run(&["commit", "-m", "isolated baseline", "--no-verify"]);

    assert_eq!(
        agent_doc_git_io::dirs::git_superproject_at(doc.parent().unwrap()),
        None,
        "a GIT_DIR override shares the 'superproject' git dir and must not be a submodule"
    );
    let drift = agent_doc_git_io::submodule::submodule_pointer_drift(&doc).unwrap();
    assert!(
        drift.is_none(),
        "parent-pointer drift must not be reported under a GIT_DIR override: {drift:?}"
    );
}
