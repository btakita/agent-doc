use anyhow::Result;
use std::path::Path;
use std::process::Command;

use crate::dirs::resolve_to_git_root;

/// Create and checkout the standard `agent-doc/<document-stem>` branch for a session document.
pub fn create_session_branch(file: &Path) -> Result<()> {
    let branch_name = agent_doc_git::agent_doc_branch_name_for_file(file);
    let (git_root, _) = resolve_to_git_root(file)?;
    checkout_new_or_existing(&git_root, &branch_name)
}

/// Create and check out `branch_name`, or switch to it if it already exists.
pub fn checkout_new_or_existing(git_root: &Path, branch_name: &str) -> Result<()> {
    let create = Command::new("git")
        .current_dir(git_root)
        .args(["checkout", "-b", branch_name])
        .output()?;
    if !create.status.success() {
        let checkout = Command::new("git")
            .current_dir(git_root)
            .args(["checkout", branch_name])
            .output()?;
        if !checkout.status.success() {
            anyhow::bail!(
                "failed to create or switch to branch {branch_name}: {}{}",
                String::from_utf8_lossy(&create.stderr).trim(),
                String::from_utf8_lossy(&checkout.stderr).trim()
            );
        }
    }
    Ok(())
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
        fs::write(root.join("README.md"), "body\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "init"])
            .output()
            .unwrap();
    }

    fn current_branch(root: &Path) -> String {
        let output = Command::new("git")
            .current_dir(root)
            .args(["branch", "--show-current"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    #[test]
    fn checkout_new_or_existing_creates_branch() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);

        checkout_new_or_existing(root, "agent-doc/test").unwrap();

        assert_eq!(current_branch(root), "agent-doc/test");
    }

    #[test]
    fn checkout_new_or_existing_switches_to_existing_branch() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        let initial_branch = current_branch(root);
        checkout_new_or_existing(root, "agent-doc/test").unwrap();
        checkout_new_or_existing(root, &initial_branch).unwrap();

        checkout_new_or_existing(root, "agent-doc/test").unwrap();

        assert_eq!(current_branch(root), "agent-doc/test");
    }

    #[test]
    fn create_session_branch_uses_standard_agent_doc_branch_name() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        let doc = root.join("plan.md");
        fs::write(&doc, "body\n").unwrap();

        create_session_branch(&doc).unwrap();

        assert_eq!(current_branch(root), "agent-doc/plan");
    }
}
