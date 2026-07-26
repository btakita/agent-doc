use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartialStagingDiffEvidence {
    pub repo: PathBuf,
    pub committed_paths: Vec<String>,
    pub dirty_paths: Vec<String>,
    pub committed_diff: String,
    pub dirty_diff: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartialStagingFinding {
    pub repo: PathBuf,
    pub committed_paths: Vec<String>,
    pub dirty_paths: Vec<String>,
    pub literals: Vec<String>,
}

pub fn companion_findings(file: &Path) -> Result<Vec<PartialStagingFinding>> {
    let mut findings = Vec::new();
    for repo in candidate_repos(file)? {
        let Some(evidence) = diff_evidence(&repo)? else {
            continue;
        };
        let Some(finding) = agent_doc_diff::partial_staging_companion_finding(
            &evidence.committed_paths,
            &evidence.dirty_paths,
            &evidence.committed_diff,
            &evidence.dirty_diff,
        ) else {
            continue;
        };
        findings.push(PartialStagingFinding {
            repo: evidence.repo,
            committed_paths: finding.committed_paths,
            dirty_paths: finding.dirty_paths,
            literals: finding.literals,
        });
    }
    Ok(findings)
}

pub fn candidate_repos(file: &Path) -> Result<Vec<PathBuf>> {
    let start = if file.is_dir() {
        file
    } else {
        file.parent().unwrap_or_else(|| Path::new("."))
    };
    let Some(root) = git_toplevel(start)? else {
        return Ok(Vec::new());
    };

    let mut repos = vec![root.clone()];
    if let Some(status) = git_stdout(
        &root,
        &["status", "--porcelain=v1", "--ignore-submodules=none"],
    )? {
        for line in status.lines() {
            let Some(rel) = agent_doc_git::parse_porcelain_path(line) else {
                continue;
            };
            let candidate = root.join(rel);
            if !candidate.is_dir() {
                continue;
            }
            if let Some(subroot) = git_toplevel(&candidate)?
                && subroot != root
            {
                repos.push(subroot);
            }
        }
    }

    repos.sort();
    repos.dedup();
    Ok(repos)
}


/// Deduplicated pathspec operands for a diff, so a path listed twice (dirty and
/// staged) is not passed twice.
fn pathspec_args(paths: &[String]) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    paths
        .iter()
        .filter(|path| seen.insert((*path).clone()))
        .cloned()
        .collect()
}

/// `base` followed by `-- <paths>`, or `base` alone when there are no paths.
fn diff_args<'a>(base: &[&'a str], paths: &'a [String]) -> Vec<&'a str> {
    let mut args: Vec<&str> = base.to_vec();
    if !paths.is_empty() {
        args.push("--");
        args.extend(paths.iter().map(String::as_str));
    }
    args
}

pub fn diff_evidence(repo: &Path) -> Result<Option<PartialStagingDiffEvidence>> {
    if git_stdout(repo, &["rev-parse", "--verify", "HEAD^"])?.is_none() {
        return Ok(None);
    }

    let committed_paths = git_name_lines(
        repo,
        &[
            "diff",
            "--name-only",
            "--diff-filter=ACMRT",
            "HEAD^",
            "HEAD",
        ],
    )?;

    let mut dirty_paths = git_name_lines(repo, &["diff", "--name-only", "--diff-filter=ACMRT"])?;
    dirty_paths.extend(git_name_lines(
        repo,
        &["diff", "--cached", "--name-only", "--diff-filter=ACMRT"],
    )?);

    // A partial-staging finding needs BOTH a committed change and an uncommitted
    // companion; `partial_staging_companion_finding` returns `None` the moment
    // either side is empty. The three `--unified=0` diffs below are the whole
    // cost of this guard -- full patch text for the repo -- so paying them to
    // then discard the result is pure waste (`#idlerevisionreactive`: gate the
    // expensive work on the cheap probe that already ran).
    //
    // On a superproject with ~25 submodules, most repos have one side empty on
    // any given sweep, so this skips the expensive diffs for nearly all of them.
    if committed_paths.is_empty() || dirty_paths.is_empty() {
        return Ok(None);
    }

    // Scope each diff to the paths it will actually be read for. The finding is
    // computed from `committed_paths`/`dirty_paths` and the hunks belonging to
    // them, so a whole-repo patch just produces text that is filtered away --
    // and on a repo with many dirty files that text is the bulk of the guard's
    // runtime.
    let committed_pathspec = pathspec_args(&committed_paths);
    let dirty_pathspec = pathspec_args(&dirty_paths);

    let committed_diff = git_stdout(
        repo,
        &diff_args(&["diff", "--unified=0", "HEAD^", "HEAD"], &committed_pathspec),
    )?
    .unwrap_or_default();
    let mut dirty_diff =
        git_stdout(repo, &diff_args(&["diff", "--unified=0"], &dirty_pathspec))?.unwrap_or_default();
    if let Some(cached) = git_stdout(
        repo,
        &diff_args(&["diff", "--cached", "--unified=0"], &dirty_pathspec),
    )? {
        if !dirty_diff.is_empty() && !cached.is_empty() {
            dirty_diff.push('\n');
        }
        dirty_diff.push_str(&cached);
    }

    Ok(Some(PartialStagingDiffEvidence {
        repo: repo.to_path_buf(),
        committed_paths,
        dirty_paths,
        committed_diff,
        dirty_diff,
    }))
}

fn git_toplevel(start: &Path) -> Result<Option<PathBuf>> {
    let Some(stdout) = git_stdout(start, &["rev-parse", "--show-toplevel"])? else {
        return Ok(None);
    };
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    Ok(Some(PathBuf::from(trimmed)))
}

fn git_name_lines(repo: &Path, args: &[&str]) -> Result<Vec<String>> {
    let Some(stdout) = git_stdout(repo, args)? else {
        return Ok(Vec::new());
    };
    Ok(stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect())
}

fn git_stdout(repo: &Path, args: &[&str]) -> Result<Option<String>> {
    let output = Command::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .with_context(|| format!("failed to run git {:?} in {}", args, repo.display()))?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(String::from_utf8_lossy(&output.stdout).to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn git(cwd: &Path, args: &[&str]) {
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

    fn init_repo(root: &Path) {
        git(root, &["init"]);
        git(root, &["config", "user.email", "test@example.com"]);
        git(root, &["config", "user.name", "Test"]);
    }

    #[test]
    fn diff_evidence_collects_committed_and_staged_changes() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::write(
            root.join("src/render.rs"),
            "pub fn render() -> &'static str { \"old queue output\" }\n",
        )
        .unwrap();
        fs::write(
            root.join("tests/render_test.rs"),
            "#[test]\nfn render_output() { assert_eq!(agent::render(), \"old queue output\"); }\n",
        )
        .unwrap();
        git(root, &["add", "src/render.rs", "tests/render_test.rs"]);
        git(root, &["commit", "-m", "initial", "--no-verify"]);

        fs::write(
            root.join("src/render.rs"),
            "pub fn render() -> &'static str { \"new queue output\" }\n",
        )
        .unwrap();
        git(root, &["add", "src/render.rs"]);
        git(root, &["commit", "-m", "source only", "--no-verify"]);

        fs::write(
            root.join("tests/render_test.rs"),
            "#[test]\nfn render_output() { assert_eq!(agent::render(), \"new queue output\"); }\n",
        )
        .unwrap();
        git(root, &["add", "tests/render_test.rs"]);

        let evidence = diff_evidence(root).unwrap().unwrap();
        assert_eq!(evidence.repo, root);
        assert_eq!(evidence.committed_paths, ["src/render.rs"]);
        assert_eq!(evidence.dirty_paths, ["tests/render_test.rs"]);
        assert!(evidence.committed_diff.contains("new queue output"));
        assert!(evidence.dirty_diff.contains("new queue output"));
    }

    #[test]
    fn companion_findings_detects_dirty_companion_literal() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::write(
            root.join("src/render.rs"),
            "pub fn render() -> &'static str { \"old queue output\" }\n",
        )
        .unwrap();
        fs::write(
            root.join("tests/render_test.rs"),
            "#[test]\nfn render_output() { assert_eq!(agent::render(), \"old queue output\"); }\n",
        )
        .unwrap();
        git(root, &["add", "src/render.rs", "tests/render_test.rs"]);
        git(root, &["commit", "-m", "initial", "--no-verify"]);

        fs::write(
            root.join("src/render.rs"),
            "pub fn render() -> &'static str { \"new queue output\" }\n",
        )
        .unwrap();
        git(root, &["add", "src/render.rs"]);
        git(root, &["commit", "-m", "source only", "--no-verify"]);

        fs::write(
            root.join("tests/render_test.rs"),
            "#[test]\nfn render_output() { assert_eq!(agent::render(), \"new queue output\"); }\n",
        )
        .unwrap();

        let findings = companion_findings(&root.join("session.md")).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].repo, root);
        assert_eq!(findings[0].committed_paths, ["src/render.rs"]);
        assert_eq!(findings[0].dirty_paths, ["tests/render_test.rs"]);
        assert_eq!(
            findings[0].literals,
            ["new queue output", "old queue output"]
        );
    }

    #[test]
    fn diff_evidence_returns_none_without_parent_commit() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        fs::write(root.join("doc.md"), "body\n").unwrap();
        git(root, &["add", "doc.md"]);
        git(root, &["commit", "-m", "initial", "--no-verify"]);

        assert_eq!(diff_evidence(root).unwrap(), None);
    }

    #[test]
    fn candidate_repos_includes_nested_git_worktree_from_status() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        let nested = root.join("nested");
        fs::create_dir_all(&nested).unwrap();
        init_repo(&nested);
        fs::write(nested.join("inner.txt"), "dirty\n").unwrap();
        git(&nested, &["add", "inner.txt"]);
        git(&nested, &["commit", "-m", "nested initial", "--no-verify"]);
        git(root, &["add", "nested"]);

        let repos = candidate_repos(&root.join("doc.md")).unwrap();
        assert!(repos.contains(&root.to_path_buf()), "{repos:?}");
        assert!(repos.contains(&nested), "{repos:?}");
    }
}
