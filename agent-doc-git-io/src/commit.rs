use anyhow::Result;
use std::path::Path;
use std::process::{Command, Output};

pub fn commit_no_verify(git_root: &Path, message: &str) -> Result<Output> {
    Ok(Command::new("git")
        .current_dir(git_root)
        .args(["commit", "-m", message, "--no-verify"])
        .output()?)
}

pub fn commit_no_verify_pathspec(
    git_root: &Path,
    message: &str,
    rel_path: &Path,
) -> Result<Output> {
    Ok(Command::new("git")
        .current_dir(git_root)
        .args(["commit", "-m", message, "--no-verify", "--"])
        .arg(rel_path)
        .output()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn init_repo(root: &Path) {
        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
    }

    #[test]
    fn commit_no_verify_commits_staged_changes() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        fs::write(root.join("doc.md"), "body\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();

        let output = commit_no_verify(root, "init").unwrap();
        assert!(output.status.success());
    }

    #[test]
    fn commit_no_verify_pathspec_commits_path() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        fs::write(root.join("doc.md"), "body\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();

        let output = commit_no_verify_pathspec(root, "init", Path::new("doc.md")).unwrap();
        assert!(output.status.success());
    }
}
