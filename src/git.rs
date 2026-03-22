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
    let t_total = std::time::Instant::now();

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
    let t_staging = std::time::Instant::now();
    if let Some(ref snap) = snapshot_content {
        // Add (HEAD) marker to the last ### Re: heading in the snapshot.
        // The working tree keeps the heading WITHOUT the marker, creating
        // a single modified line (blue gutter) as a visual boundary.
        let staged_content = add_head_marker(snap, file);

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
    let elapsed_staging = t_staging.elapsed().as_millis();
    if elapsed_staging > 0 {
        eprintln!("[perf] commit.staging (hash_object+update-index): {}ms", elapsed_staging);
    }

    // Commit — ignore failure (nothing to commit is fine).
    // Use .output() to capture stdout (prevents git status leaking to stdout
    // when called from `preflight` which reserves stdout for JSON).
    let t_commit = std::time::Instant::now();
    let commit_output = Command::new("git")
        .current_dir(&git_root)
        .args(["commit", "-m", &msg, "--no-verify"])
        .output();
    let commit_status = commit_output.as_ref().map(|o| o.status);
    let elapsed_commit = t_commit.elapsed().as_millis();
    if elapsed_commit > 0 {
        eprintln!("[perf] commit.git_commit: {}ms", elapsed_commit);
    }

    // Log commit output to stderr (captured from git to avoid stdout pollution)
    if let Ok(ref o) = commit_output {
        let stdout = String::from_utf8_lossy(&o.stdout);
        for line in stdout.lines() {
            if !line.trim().is_empty() {
                eprintln!("{}", line);
            }
        }
    }

    // After commit, strip (HEAD) markers from the snapshot so the working tree
    // is clean. The committed content has (HEAD) markers; the working tree should not.
    // This creates the blue gutter diff the user sees.
    if let Ok(ref s) = commit_status
        && s.success()
    {
            // Strip (HEAD) from snapshot
            if let Some(ref snap) = snapshot_content {
                let clean_snap = strip_head_markers(snap);
                if clean_snap != *snap {
                    if let Err(e) = crate::snapshot::save(file, &clean_snap) {
                        eprintln!("[commit] failed to clean snapshot: {}", e);
                    }
                }
            }
            // Note: working tree is NOT modified here. The staged content has (HEAD)
            // markers, the working tree does not. This creates the blue gutter diff.
            // Previously we stripped HEAD markers from the working tree, but that was
            // unnecessary (staging doesn't modify the working tree) and caused file
            // cache conflicts in the IDE.

            // Reposition boundary in snapshot AND via IPC to the plugin.
            // Working tree is NOT written directly (would lose user keystrokes).
            // The IPC signal tells the plugin to reposition in its Document buffer.
            let t_reposition = std::time::Instant::now();
            reposition_boundary_in_snapshot(file);
            // Send IPC reposition signal to plugin (non-blocking, non-fatal)
            crate::write::try_ipc_reposition_boundary(file);
            let elapsed_reposition = t_reposition.elapsed().as_millis();
            if elapsed_reposition > 0 {
                eprintln!("[perf] commit.reposition: {}ms", elapsed_reposition);
            }
    }

    let elapsed_total = t_total.elapsed().as_millis();
    if elapsed_total > 0 {
        eprintln!("[perf] commit total: {}ms", elapsed_total);
    }

    Ok(())
}

/// Reposition boundary in snapshot only (not working tree).
///
/// After commit, moves the boundary to the end of exchange in the snapshot.
/// The working tree is NOT modified — writing to it while the user is typing
/// causes the IDE to reload from disk, losing in-progress keystrokes.
/// The plugin handles working-tree boundary reposition via the
/// `reposition_boundary: true` IPC flag sent during `agent-doc write`.
fn reposition_boundary_in_snapshot(file: &Path) {
    // Check for active run — don't reposition if a run is in progress
    if let Ok(canonical) = file.canonicalize() {
        if let Ok(pending_path) = crate::snapshot::pending_path_for(&canonical) {
            if pending_path.exists() {
                eprintln!("[commit] skipping boundary reposition — active run detected");
                return;
            }
        }
    }

    // Reposition in snapshot only
    if let Ok(Some(snap_content)) = crate::snapshot::load(file) {
        if let Some(new_snap) = move_boundary_to_end_in(&snap_content) {
            if let Err(e) = crate::snapshot::save(file, &new_snap) {
                eprintln!("[commit] failed to update snapshot after boundary reposition: {}", e);
            } else {
                eprintln!("[commit] repositioned boundary in snapshot");
            }
        }
    }
}

/// Move boundary marker to end of exchange in a content string.
/// Returns `Some(new_content)` if moved, `None` if already at end or no boundary.
pub(crate) fn move_boundary_to_end_in(content: &str) -> Option<String> {
    let components = crate::component::parse(content).ok()?;
    let exchange = components.iter().find(|c| c.name == "exchange")?;

    let comp_content = &content[exchange.open_end..exchange.close_start];
    let prefix = "<!-- agent:boundary:";
    let suffix = " -->";
    let code_ranges = crate::component::find_code_ranges(content);
    let in_code = |pos: usize| code_ranges.iter().any(|&(s, e)| pos >= s && pos < e);

    let mut boundary_line_start = None;
    let mut boundary_line_end = None;
    let mut found = false;

    let mut offset = exchange.open_end;
    for line in comp_content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(prefix) && trimmed.ends_with(suffix) && !in_code(offset) {
            boundary_line_start = Some(offset);
            boundary_line_end = Some(offset + line.len() + 1);
            found = true;
        }
        offset += line.len() + 1;
    }

    if !found {
        return None;
    }

    let bls = boundary_line_start?;
    let ble = boundary_line_end?;

    // Check if already at end
    let after_boundary = content[ble..exchange.close_start].trim();
    if after_boundary.is_empty() {
        return None;
    }

    // Remove from current position and re-insert at end
    let new_id = uuid::Uuid::new_v4().to_string();
    let marker = format!("<!-- agent:boundary:{} -->", new_id);
    let mut result = String::with_capacity(content.len() + marker.len());
    result.push_str(&content[..bls]);
    result.push_str(&content[ble..exchange.close_start]);
    if !result.ends_with('\n') {
        result.push('\n');
    }
    result.push_str(&marker);
    result.push('\n');
    result.push_str(&content[exchange.close_start..]);

    Some(result)
}

/// Strip ` (HEAD)` suffix from markdown heading lines only.
fn strip_head_markers(content: &str) -> String {
    let mut lines: Vec<&str> = Vec::new();
    for line in content.lines() {
        lines.push(line);
    }
    let result: String = lines.iter().map(|line| {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') && line.ends_with(" (HEAD)") {
            &line[..line.len() - 7]
        } else {
            line
        }
    }).collect::<Vec<&str>>().join("\n");
    if content.ends_with('\n') { format!("{}\n", result) } else { result }
}


/// Add ` (HEAD)` suffix to ALL new markdown headings in the agent's appended content.
///
/// Matches any heading level (`#` through `######`). Compares the snapshot
/// against git HEAD to find which headings are new (added by the agent).
/// Only the top-level (shallowest) headings in the new content get marked —
/// sub-headings within a response section are left unmarked.
///
/// When git HEAD is unavailable, falls back to marking the last heading only.
fn add_head_marker(content: &str, file: &Path) -> String {
    let head_content = show_head(file).ok().flatten();

    // Step 1: Strip ALL existing (HEAD) markers from heading lines only.
    // This prevents accumulation across commit cycles.
    let mut cleaned_lines: Vec<String> = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') && trimmed.ends_with(" (HEAD)") {
            cleaned_lines.push(line[..line.len() - 7].to_string()); // strip " (HEAD)"
        } else {
            cleaned_lines.push(line.to_string());
        }
    }
    let cleaned = cleaned_lines.join("\n");
    // Preserve trailing newline
    let cleaned = if content.ends_with('\n') && !cleaned.ends_with('\n') {
        format!("{}\n", cleaned)
    } else {
        cleaned
    };

    // Also strip (HEAD) from git HEAD content for accurate comparison
    let head_cleaned = head_content.as_ref().map(|h| {
        h.lines()
            .map(|line| {
                let trimmed = line.trim_start();
                if trimmed.starts_with('#') && trimmed.ends_with(" (HEAD)") {
                    &line[..line.len() - 7]
                } else {
                    line
                }
            })
            .collect::<Vec<&str>>()
            .join("\n")
    });

    // Step 2: Collect all heading positions from cleaned content
    let mut heading_positions: Vec<(usize, usize, usize)> = Vec::new();
    let mut offset = 0usize;
    for line in cleaned.lines() {
        let trimmed = line.trim_start();
        let line_end = offset + line.len();
        if trimmed.starts_with('#') {
            let level = trimmed.chars().take_while(|c| *c == '#').count();
            if level <= 6 && trimmed.len() > level && trimmed.as_bytes()[level] == b' ' {
                heading_positions.push((offset, line_end, level));
            }
        }
        offset = line_end + 1;
    }

    if heading_positions.is_empty() {
        return cleaned;
    }

    // Step 3: Filter to headings NOT in git HEAD (= new headings from latest response)
    let new_headings: Vec<(usize, usize, usize)> = if let Some(ref hc) = head_cleaned {
        heading_positions.into_iter().filter(|(start, end, _)| {
            let heading_text = &cleaned[*start..*end];
            !hc.contains(heading_text)
        }).collect()
    } else {
        // No HEAD available → mark last heading only
        vec![*heading_positions.last().unwrap()]
    };

    if new_headings.is_empty() {
        return cleaned;
    }

    // Step 4: Only mark root-level (shallowest) headings
    let min_level = new_headings.iter().map(|(_, _, level)| *level).min().unwrap();
    let root_ends: Vec<usize> = new_headings.iter()
        .filter(|(_, _, level)| *level == min_level)
        .map(|(_, end, _)| *end)
        .collect();

    // Step 5: Insert (HEAD) markers in reverse order to preserve offsets
    let mut result = cleaned;
    for pos in root_ends.iter().rev() {
        result.insert_str(*pos, " (HEAD)");
    }
    result
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_head_markers_from_headings() {
        let input = "# Title\n### Re: Foo (HEAD)\nSome text with (HEAD) in it\n### Re: Bar (HEAD)\n";
        let result = strip_head_markers(input);
        assert_eq!(result, "# Title\n### Re: Foo\nSome text with (HEAD) in it\n### Re: Bar\n");
    }

    #[test]
    fn strip_head_markers_preserves_non_heading_lines() {
        let input = "Normal line (HEAD)\n### Heading (HEAD)\n";
        let result = strip_head_markers(input);
        assert_eq!(result, "Normal line (HEAD)\n### Heading\n");
    }

    #[test]
    fn add_head_marker_strips_old_markers() {
        let content = "### Re: Old (HEAD)\n### Re: New\n";
        let result = add_head_marker(content, Path::new("/nonexistent/file.md"));
        assert!(!result.contains("### Re: Old (HEAD)"), "old heading should not have (HEAD)");
        assert!(result.contains("### Re: New (HEAD)") || result.contains("### Re: Old\n"), "old (HEAD) should be stripped");
    }

    #[test]
    fn move_boundary_to_end_basic() {
        let content = "<!-- agent:exchange patch=append -->\nResponse.\n<!-- agent:boundary:abc123 -->\nUser prompt.\n<!-- /agent:exchange -->\n";
        let result = move_boundary_to_end_in(content).unwrap();
        // Boundary should be after user prompt, before close tag
        assert!(result.contains("User prompt.\n<!-- agent:boundary:"));
        assert!(result.contains("-->\n<!-- /agent:exchange -->"));
        // Old boundary consumed
        assert!(!result.contains("abc123"));
    }

    #[test]
    fn move_boundary_already_at_end() {
        let content = "<!-- agent:exchange patch=append -->\nResponse.\nUser prompt.\n<!-- agent:boundary:abc123 -->\n<!-- /agent:exchange -->\n";
        let result = move_boundary_to_end_in(content);
        assert!(result.is_none(), "should return None when boundary is already at end");
    }

    #[test]
    fn move_boundary_no_exchange() {
        let content = "# No exchange component\nJust text.\n";
        let result = move_boundary_to_end_in(content);
        assert!(result.is_none());
    }

    #[test]
    fn move_boundary_preserves_user_edits() {
        // Simulates: agent response committed, user typed below boundary
        let content = "<!-- agent:exchange patch=append -->\n### Re: Answer\nAgent response.\n<!-- agent:boundary:old-id -->\nUser's new prompt here.\nMore user text.\n<!-- /agent:exchange -->\n";
        let result = move_boundary_to_end_in(content).unwrap();
        // User text should be preserved and above the new boundary
        assert!(result.contains("User's new prompt here."), "user edit must be preserved");
        assert!(result.contains("More user text."), "user edit must be preserved");
        // New boundary should be after user text
        let boundary_pos = result.find("<!-- agent:boundary:").unwrap();
        let user_pos = result.find("User's new prompt here.").unwrap();
        assert!(boundary_pos > user_pos, "boundary must be after user text");
    }
}
