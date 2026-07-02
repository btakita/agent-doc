use anyhow::Result;
use std::path::Path;
use std::process::Command;

use crate::commit::commit_no_verify;
use crate::dirs::resolve_to_git_root;

/// Squash all agent-doc commits touching a session document into one.
pub fn squash_session(file: &Path) -> Result<()> {
    let (git_root, resolved) = resolve_to_git_root(file)?;
    let pathspec = agent_doc_git::relative_to_root(&resolved, &git_root);
    let message = format!("agent-doc: squashed session for {}", file.display());
    match squash_agent_doc_commits(&git_root, &pathspec, &message)? {
        SquashOutcome::NoAgentDocCommits => {
            eprintln!("No agent-doc commits found for {}", file.display());
        }
        SquashOutcome::Squashed => {
            eprintln!("Squashed agent-doc commits for {}", file.display());
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SquashOutcome {
    Squashed,
    NoAgentDocCommits,
}

pub fn squash_agent_doc_commits(
    git_root: &Path,
    pathspec: &Path,
    message: &str,
) -> Result<SquashOutcome> {
    let output = Command::new("git")
        .current_dir(git_root)
        .args(["log", "--oneline", "--reverse", "--grep=^agent-doc", "--"])
        .arg(pathspec)
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let Some(first_line) = stdout.lines().next() else {
        return Ok(SquashOutcome::NoAgentDocCommits);
    };
    let first_hash = first_line.split_whitespace().next().unwrap_or("");

    let status = Command::new("git")
        .current_dir(git_root)
        .args(["reset", "--soft", &format!("{first_hash}~1")])
        .status()?;
    if !status.success() {
        anyhow::bail!("git reset failed");
    }

    let output = commit_no_verify(git_root, message)?;
    if !output.status.success() {
        anyhow::bail!("git commit failed during squash");
    }

    Ok(SquashOutcome::Squashed)
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

    fn commit_file(root: &Path, rel: &str, content: &str, message: &str) {
        fs::write(root.join(rel), content).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", rel])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", message])
            .output()
            .unwrap();
    }

    fn log_subjects(root: &Path) -> Vec<String> {
        let output = Command::new("git")
            .current_dir(root)
            .args(["log", "--format=%s", "--reverse"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn squash_agent_doc_commits_combines_matching_commits() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        commit_file(root, "doc.md", "initial\n", "initial");
        commit_file(root, "doc.md", "one\n", "agent-doc(doc): one");
        commit_file(root, "doc.md", "two\n", "agent-doc(doc): two");

        let outcome =
            squash_agent_doc_commits(root, Path::new("doc.md"), "agent-doc: squashed").unwrap();

        assert_eq!(outcome, SquashOutcome::Squashed);
        assert_eq!(log_subjects(root), vec!["initial", "agent-doc: squashed"]);
    }

    #[test]
    fn squash_agent_doc_commits_reports_no_matching_commits() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        commit_file(root, "doc.md", "initial\n", "initial");

        let outcome =
            squash_agent_doc_commits(root, Path::new("doc.md"), "agent-doc: squashed").unwrap();

        assert_eq!(outcome, SquashOutcome::NoAgentDocCommits);
        assert_eq!(log_subjects(root), vec!["initial"]);
    }

    #[test]
    fn squash_session_resolves_document_and_squashes_agent_doc_commits() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        commit_file(root, "doc.md", "initial\n", "initial");
        commit_file(root, "doc.md", "one\n", "agent-doc(doc): one");
        commit_file(root, "doc.md", "two\n", "agent-doc(doc): two");

        squash_session(&root.join("doc.md")).unwrap();

        assert_eq!(
            log_subjects(root),
            vec![
                "initial".to_string(),
                format!(
                    "agent-doc: squashed session for {}",
                    root.join("doc.md").display()
                )
            ]
        );
    }
}
