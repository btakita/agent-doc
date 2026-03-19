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
    let doc_name = file
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    let msg = format!("agent-doc({}): {}", doc_name, timestamp);

    // Selective commit: stage only the snapshot content (agent response),
    // leaving user edits in the working tree as uncommitted.
    //
    // If a snapshot exists, use git hash-object + update-index to stage the
    // snapshot version without touching the working tree file. This means:
    // - Agent response → committed (no git gutter)
    // - User's subsequent edits → uncommitted (green git gutter)
    let snapshot_content = crate::snapshot::load(file)?;
    if let Some(ref snap) = snapshot_content {
        // Add (HEAD) marker to the last ### Re: heading in the snapshot.
        // The working tree keeps the heading WITHOUT the marker, creating
        // a single modified line (blue gutter) as a visual boundary.
        let staged_content = add_head_marker(snap);

        let rel_path = resolved.strip_prefix(&git_root)
            .unwrap_or(&resolved);

        if let Ok(hash) = hash_object(&git_root, &staged_content) {
            let cacheinfo = format!("100644,{},{}", hash, rel_path.to_string_lossy());
            let status = Command::new("git")
                .current_dir(&git_root)
                .args(["update-index", "--add", "--cacheinfo", &cacheinfo])
                .status()?;
            if !status.success() {
                eprintln!("[commit] update-index failed, falling back to git add");
                git_add_force(&git_root, &resolved)?;
            }
        } else {
            git_add_force(&git_root, &resolved)?;
        }
    } else {
        // No snapshot — fall back to staging the entire file
        git_add_force(&git_root, &resolved)?;
    }

    // Commit — ignore failure (nothing to commit is fine)
    let _ = Command::new("git")
        .current_dir(&git_root)
        .args(["commit", "-m", &msg, "--no-verify"])
        .status();
    Ok(())
}

/// Add ` (HEAD)` suffix to the last `### ` heading in the content.
///
/// This creates a single modified line in the git diff between the committed
/// version (with marker) and the working tree (without marker), serving as
/// a visual boundary in the editor's git gutter.
///
/// Matches any `### ` heading (not just `### Re:`) since agent responses
/// may use different heading formats across documents.
fn add_head_marker(content: &str) -> String {
    // Find the last "### " heading (any h3)
    if let Some(last_pos) = content.rfind("\n### ") {
        let line_start = last_pos + 1; // skip the newline
        let line_end = content[line_start..].find('\n')
            .map(|i| line_start + i)
            .unwrap_or(content.len());
        let heading = &content[line_start..line_end];

        // Don't double-add the marker
        if heading.ends_with(" (HEAD)") {
            return content.to_string();
        }

        let mut result = String::with_capacity(content.len() + 7);
        result.push_str(&content[..line_end]);
        result.push_str(" (HEAD)");
        result.push_str(&content[line_end..]);
        result
    } else {
        content.to_string()
    }
}

/// Write content to git's object database and return the blob hash.
fn hash_object(git_root: &Path, content: &str) -> Result<String> {
    let output = Command::new("git")
        .current_dir(git_root)
        .args(["hash-object", "-w", "--stdin"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(ref mut stdin) = child.stdin {
                stdin.write_all(content.as_bytes())?;
            }
            child.wait_with_output()
        })?;
    if !output.status.success() {
        anyhow::bail!("git hash-object failed");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Force-add a file to the index (fallback path).
fn git_add_force(git_root: &Path, resolved: &Path) -> Result<()> {
    let status = Command::new("git")
        .current_dir(git_root)
        .args(["add", "-f", &resolved.to_string_lossy()])
        .status()?;
    if !status.success() {
        anyhow::bail!("git add failed");
    }
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
            "--grep=^agent-doc",
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

/// Get the content of a file from the last agent-doc commit (or HEAD).
/// Returns None if the file is not tracked or no commits exist.
pub fn show_head(file: &Path) -> Result<Option<String>> {
    let (git_root, resolved) = resolve_to_git_root(file)?;

    // Get the file path relative to the git root
    let rel_path = if resolved.is_absolute() {
        resolved
            .strip_prefix(&git_root)
            .unwrap_or(&resolved)
            .to_path_buf()
    } else {
        resolved.clone()
    };

    let output = Command::new("git")
        .current_dir(&git_root)
        .args(["show", &format!("HEAD:{}", rel_path.to_string_lossy())])
        .output()?;

    if !output.status.success() {
        // File not tracked or no commits — not an error
        return Ok(None);
    }

    Ok(Some(String::from_utf8_lossy(&output.stdout).to_string()))
}

/// Get the author timestamp of the last commit touching a file.
/// Returns None if the file has no commits.
pub fn last_commit_mtime(file: &Path) -> Result<Option<std::time::SystemTime>> {
    let (git_root, resolved) = resolve_to_git_root(file)?;

    let rel_path = if resolved.is_absolute() {
        resolved
            .strip_prefix(&git_root)
            .unwrap_or(&resolved)
            .to_path_buf()
    } else {
        resolved.clone()
    };

    let output = Command::new("git")
        .current_dir(&git_root)
        .args([
            "log",
            "-1",
            "--format=%ct",
            "--",
            &rel_path.to_string_lossy(),
        ])
        .output()?;

    if !output.status.success() {
        return Ok(None);
    }

    let ts_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if ts_str.is_empty() {
        return Ok(None);
    }

    let epoch: u64 = ts_str.parse().unwrap_or(0);
    if epoch == 0 {
        return Ok(None);
    }

    Ok(Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(epoch)))
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
