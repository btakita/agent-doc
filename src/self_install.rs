//! Install the current committed checkout from an isolated sibling worktree.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn run(
    source_root: Option<&Path>,
    target_dir: Option<&Path>,
    keep_worktree: bool,
    profile: &str,
) -> Result<()> {
    let repo_root = match source_root {
        Some(path) => path
            .canonicalize()
            .with_context(|| format!("canonicalize source root {}", path.display()))?,
        None => current_git_root()?,
    };

    let worktree = IsolatedWorktree::create(&repo_root, keep_worktree)?;
    eprintln!("[self-install] worktree: {}", worktree.path().display());

    let install_label = format!("cargo install --path . --profile {profile}");
    run_command(
        worktree.path(),
        "cargo",
        &["install", "--path", ".", "--profile", profile],
        &install_label,
    )?;
    let lib_build_label = format!("cargo build --profile {profile} --lib");
    run_command(
        worktree.path(),
        "cargo",
        &["build", "--profile", profile, "--lib"],
        &lib_build_label,
    )?;

    let lib_source = profile_lib_path(worktree.path(), profile);
    if !lib_source.exists() {
        bail!(
            "[self-install] built shared library not found: {}",
            lib_source.display()
        );
    }
    crate::lib_install::run_paths(Some(&lib_source), target_dir, profile)?;

    if keep_worktree {
        eprintln!(
            "[self-install] kept worktree at {}",
            worktree.path().display()
        );
    } else {
        worktree.cleanup()?;
    }

    Ok(())
}

struct IsolatedWorktree {
    repo_root: PathBuf,
    path: Option<PathBuf>,
    keep: bool,
}

impl IsolatedWorktree {
    fn create(repo_root: &Path, keep: bool) -> Result<Self> {
        let path = build_worktree_path(repo_root)?;
        if path.exists() {
            bail!(
                "[self-install] worktree path already exists: {}",
                path.display()
            );
        }

        let output = Command::new("git")
            .current_dir(repo_root)
            .args(["worktree", "add", "--detach"])
            .arg(&path)
            .arg("HEAD")
            .output()
            .context("spawn git worktree add")?;

        if !output.status.success() {
            bail!(
                "[self-install] git worktree add failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        Ok(Self {
            repo_root: repo_root.to_path_buf(),
            path: Some(path),
            keep,
        })
    }

    fn path(&self) -> &Path {
        self.path.as_deref().expect("worktree path present")
    }

    fn cleanup(mut self) -> Result<()> {
        if self.keep {
            return Ok(());
        }
        let Some(path) = self.path.take() else {
            return Ok(());
        };
        remove_worktree(&self.repo_root, &path)
    }
}

impl Drop for IsolatedWorktree {
    fn drop(&mut self) {
        if self.keep {
            return;
        }
        if let Some(path) = self.path.take() {
            let _ = remove_worktree(&self.repo_root, &path);
        }
    }
}

fn current_git_root() -> Result<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("spawn git rev-parse --show-toplevel")?;
    if !output.status.success() {
        bail!(
            "[self-install] could not find git root: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let root = String::from_utf8(output.stdout).context("git root was not valid UTF-8")?;
    Ok(PathBuf::from(root.trim()))
}

fn build_worktree_path(repo_root: &Path) -> Result<PathBuf> {
    let parent = repo_root
        .parent()
        .context("source repository has no parent directory")?;
    let repo_name = repo_root
        .file_name()
        .and_then(|name| name.to_str())
        .map(sanitize_path_segment)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "repo".to_string());
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before UNIX_EPOCH")?
        .as_millis();
    Ok(parent.join(format!(
        ".agent-doc-build-{repo_name}-{}-{now}",
        std::process::id()
    )))
}

fn sanitize_path_segment(input: &str) -> String {
    input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

fn remove_worktree(repo_root: &Path, path: &Path) -> Result<()> {
    let output = Command::new("git")
        .current_dir(repo_root)
        .args(["worktree", "remove", "--force"])
        .arg(path)
        .output()
        .context("spawn git worktree remove")?;
    if !output.status.success() {
        bail!(
            "[self-install] git worktree remove failed for {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn run_command(cwd: &Path, program: &str, args: &[&str], label: &str) -> Result<()> {
    eprintln!("[self-install] {label}");
    let status = Command::new(program)
        .current_dir(cwd)
        .args(args)
        .status()
        .with_context(|| format!("spawn {label}"))?;
    if !status.success() {
        bail!("[self-install] {label} failed with status {status}");
    }
    Ok(())
}

fn profile_lib_path(worktree: &Path, profile: &str) -> PathBuf {
    crate::lib_install::profile_lib_path(worktree, None, profile)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn build_worktree_path_is_sibling_of_repo_so_path_dependencies_still_resolve() {
        let root = PathBuf::from("/tmp/workspace/src/agent-doc");
        let path = build_worktree_path(&root).unwrap();

        assert_eq!(path.parent(), root.parent());
        assert!(
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".agent-doc-build-agent-doc-")
        );
    }

    #[test]
    fn profile_lib_path_points_at_worktree_profile_cdylib() {
        let root = PathBuf::from("/tmp/worktree");
        assert_eq!(
            profile_lib_path(&root, "release-local"),
            root.join("target")
                .join("release-local")
                .join(crate::lib_install::platform_lib_name())
        );
    }

    #[test]
    fn isolated_worktree_uses_committed_head_not_dirty_checkout() {
        let dir = tempfile::TempDir::new().unwrap();
        let repo = dir.path().join("agent-doc");
        fs::create_dir(&repo).unwrap();

        git(&repo, &["init"]);
        git(&repo, &["config", "user.email", "test@example.com"]);
        git(&repo, &["config", "user.name", "Test"]);
        fs::write(repo.join("Cargo.toml"), "[package]\nname = \"clean\"\n").unwrap();
        git(&repo, &["add", "Cargo.toml"]);
        git(&repo, &["commit", "-m", "initial", "--no-verify"]);

        fs::write(
            repo.join("Cargo.toml"),
            "this dirty checkout should not build\n",
        )
        .unwrap();
        fs::write(repo.join("untracked.rs"), "broken").unwrap();

        let worktree = IsolatedWorktree::create(&repo, false).unwrap();
        let cargo_toml = fs::read_to_string(worktree.path().join("Cargo.toml")).unwrap();

        assert!(cargo_toml.contains("name = \"clean\""));
        assert!(!worktree.path().join("untracked.rs").exists());
        worktree.cleanup().unwrap();
    }

    fn git(cwd: &Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(cwd)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
