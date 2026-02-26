use anyhow::Result;
use std::path::Path;
use std::process::Command;

/// Resolve a relative path against the git root (superproject root if in a submodule).
/// Returns (git_root, resolved_file_path) so callers can run git commands in the correct repo.
fn resolve_to_git_root(file: &Path) -> Result<(std::path::PathBuf, std::path::PathBuf)> {
    if file.is_absolute() {
        // Find git root from the file's directory
        let parent = file.parent().unwrap_or(Path::new("/"));
        let root = git_toplevel_at(parent)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        return Ok((root, file.to_path_buf()));
    }

    // Try superproject first (handles submodule CWD case)
    let output = Command::new("git")
        .args(["rev-parse", "--show-superproject-working-tree"])
        .output();
    if let Ok(ref o) = output {
        let root = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if !root.is_empty() {
            let root_path = std::path::PathBuf::from(&root);
            let resolved = root_path.join(file);
            if resolved.exists() {
                return Ok((root_path, resolved));
            }
        }
    }

    // Try git toplevel
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output();
    if let Ok(ref o) = output {
        let root = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if !root.is_empty() {
            let root_path = std::path::PathBuf::from(&root);
            let resolved = root_path.join(file);
            if resolved.exists() {
                return Ok((root_path, resolved));
            }
        }
    }

    // Fallback: use as-is (relative to CWD)
    let cwd = std::env::current_dir().unwrap_or_default();
    Ok((cwd, file.to_path_buf()))
}

/// Get git toplevel from a specific directory.
fn git_toplevel_at(dir: &Path) -> Option<std::path::PathBuf> {
    Command::new("git")
        .current_dir(dir)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .and_then(|o| {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if s.is_empty() { None } else { Some(std::path::PathBuf::from(s)) }
        })
}

/// Commit a file with an auto-generated message. Skips hooks.
/// Relative paths are resolved against the git root (superproject if in a submodule).
/// Git commands run from the resolved git root, so this works even when CWD is a submodule.
pub fn commit(file: &Path) -> Result<()> {
    let (git_root, resolved) = resolve_to_git_root(file)?;
    let timestamp = chrono_timestamp();
    let msg = format!("agent-doc: {}", timestamp);

    let status = Command::new("git")
        .current_dir(&git_root)
        .args(["add", "-f", &resolved.to_string_lossy()])
        .status()?;
    if !status.success() {
        anyhow::bail!("git add failed");
    }

    // Commit — ignore failure (nothing to commit is fine)
    let _ = Command::new("git")
        .current_dir(&git_root)
        .args(["commit", "-m", &msg, "--no-verify"])
        .status();
    Ok(())
}

/// Create and checkout a branch for the session.
pub fn create_branch(file: &Path) -> Result<()> {
    let stem = file
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "session".to_string());
    let branch_name = format!("agent-doc/{}", stem);

    let status = Command::new("git")
        .args(["checkout", "-b", &branch_name])
        .status()?;
    if !status.success() {
        // Branch may already exist — try switching to it
        let status = Command::new("git")
            .args(["checkout", &branch_name])
            .status()?;
        if !status.success() {
            anyhow::bail!("failed to create or switch to branch {}", branch_name);
        }
    }
    Ok(())
}

/// Squash all agent-doc commits touching a file into one.
pub fn squash_session(file: &Path) -> Result<()> {
    let file_str = file.to_string_lossy();

    // Find the first agent-doc commit for this file
    let output = Command::new("git")
        .args([
            "log",
            "--oneline",
            "--reverse",
            "--grep=agent-doc:",
            "--",
            &file_str,
        ])
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().next();
    let first_hash = match first_line {
        Some(line) => line.split_whitespace().next().unwrap_or(""),
        None => {
            eprintln!("No agent-doc commits found for {}", file.display());
            return Ok(());
        }
    };

    // Soft reset to the commit before the first agent-doc commit
    let status = Command::new("git")
        .args(["reset", "--soft", &format!("{}~1", first_hash)])
        .status()?;
    if !status.success() {
        anyhow::bail!("git reset failed");
    }

    // Recommit as a single squashed commit
    let status = Command::new("git")
        .args([
            "commit",
            "-m",
            &format!("agent-doc: squashed session for {}", file.display()),
            "--no-verify",
        ])
        .status()?;
    if !status.success() {
        anyhow::bail!("git commit failed during squash");
    }

    eprintln!("Squashed agent-doc commits for {}", file.display());
    Ok(())
}

fn chrono_timestamp() -> String {
    // Use date command for simplicity — no extra dependency
    let output = Command::new("date")
        .args(["+%Y-%m-%d %H:%M:%S"])
        .output()
        .ok();
    match output {
        Some(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        None => "unknown".to_string(),
    }
}
