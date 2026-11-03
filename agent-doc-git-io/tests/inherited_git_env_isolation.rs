//! `#testgitenvisolation`: a test process must never inherit Git's
//! repository-locating environment.
//!
//! `git bisect run`, `git rebase --exec`, and `!` aliases export an absolute
//! `GIT_DIR` when started inside a submodule or linked worktree. A fixture that
//! then runs `git init` / `git config user.name` in its tempdir reinitializes
//! the INHERITED repository instead: on 2026-11-02 this set `core.bare = true`
//! and `user.name = Test User` in the shared agent-doc submodule config.
//!
//! The `.cargo/config.toml` runner removes these variables before any test
//! binary starts. Run plainly this test checks fixture hygiene; `make
//! git-env-isolation-self-test` runs it with `GIT_DIR` aimed at a sentinel
//! repository and proves the sentinel's config is untouched.

use std::path::Path;
use std::process::Command;

/// Git's `local_repo_env` (`git rev-parse --local-env-vars`).
fn local_repo_env() -> Vec<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--local-env-vars"])
        .output()
        .expect("run git rev-parse --local-env-vars");
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(str::to_string)
        .filter(|name| !name.is_empty())
        .collect()
}

fn git(cwd: &Path, args: &[&str]) {
    // Deliberately no `env_remove`: this is the shape of the ~1000 fixture
    // call sites the runner protects.
    let output = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed in {}: {}",
        cwd.display(),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_processes_never_inherit_a_git_repository_override() {
    let inherited = local_repo_env()
        .into_iter()
        .filter_map(|name| std::env::var_os(&name).map(|value| format!("{name}={value:?}")))
        .collect::<Vec<_>>();
    assert!(
        inherited.is_empty(),
        "test process inherited Git repository environment {inherited:?}; \
         a fixture's `git init`/`git config` would rewrite that repository. \
         Run tests through cargo (the .cargo/config.toml runner) or `env -u` the \
         `git rev-parse --local-env-vars` list (#testgitenvisolation)"
    );

    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["config", "user.name", "Test User"]);
    git(dir.path(), &["config", "user.email", "test@example.com"]);
    let config = std::fs::read_to_string(dir.path().join(".git/config"))
        .expect("fixture `git init` must create the tempdir's own repository");
    assert!(config.contains("Test User"), "{config}");
    assert!(config.contains("bare = false"), "{config}");
}
