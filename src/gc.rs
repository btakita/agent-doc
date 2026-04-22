//! # Module: gc
//!
//! ## Spec
//! - Garbage-collects orphaned files in `.agent-doc/` directories.
//! - Scans snapshots, crdt, pre-response, locks, and baselines for hash-keyed files.
//! - Cross-references hashes against existing documents in the project.
//! - Removes files whose corresponding document no longer exists.
//! - `--dry-run` flag shows what would be deleted without deleting.
//!
//! ## Agentic Contracts
//! - `run(root, dry_run)` — scans `.agent-doc/` under `root`, removes orphaned files.
//!   Returns `Ok(GcResult)` with counts of deleted/skipped files.
//! - Never deletes files for documents that still exist on disk.
//! - Stale lock files (>1 hour old) are always cleaned regardless of document existence.

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::snapshot;

pub struct GcResult {
    pub deleted: usize,
    pub skipped: usize,
}

/// Run garbage collection on `.agent-doc/` directories.
///
/// `root` is the project root containing `.agent-doc/`.
/// If `None`, walks up from CWD to find it.
pub fn run(root: Option<&Path>, dry_run: bool) -> Result<GcResult> {
    let project_root = match root {
        Some(r) => r.to_path_buf(),
        None => find_project_root_from_cwd()?,
    };

    let agent_doc_dir = project_root.join(".agent-doc");
    if !agent_doc_dir.is_dir() {
        anyhow::bail!(".agent-doc/ directory not found in {}", project_root.display());
    }

    // Collect hashes of all existing documents
    let known_hashes = collect_document_hashes(&project_root)?;
    eprintln!("[gc] Found {} tracked documents", known_hashes.len());

    let mut total_deleted = 0;
    let mut total_skipped = 0;

    // Clean each hash-keyed directory
    for (dir_name, extensions) in &[
        ("snapshots", vec!["md"]),
        ("crdt", vec!["yrs"]),
        ("pre-response", vec!["md"]),
        ("baselines", vec!["md"]),
        ("annotations", vec!["json"]),
    ] {
        let dir = agent_doc_dir.join(dir_name);
        if !dir.is_dir() {
            continue;
        }
        let (deleted, skipped) = clean_orphaned_files(&dir, extensions, &known_hashes, dry_run)?;
        if deleted > 0 || skipped > 0 {
            eprintln!("[gc] {}: {} deleted, {} kept", dir_name, deleted, skipped);
        }
        total_deleted += deleted;
        total_skipped += skipped;
    }

    // Clean stale lock files (locks/ and crdt/*.lock)
    let (lock_deleted, lock_kept) = clean_stale_locks(&agent_doc_dir, dry_run)?;
    if lock_deleted > 0 {
        eprintln!("[gc] locks: {} stale deleted, {} kept", lock_deleted, lock_kept);
    }
    total_deleted += lock_deleted;
    total_skipped += lock_kept;

    // Clean hook event files (hooks/post_write/, hooks/post_commit/)
    let (hook_deleted, hook_kept) = clean_old_hooks(&agent_doc_dir, dry_run)?;
    if hook_deleted > 0 {
        eprintln!("[gc] hooks: {} old events deleted, {} kept", hook_deleted, hook_kept);
    }
    total_deleted += hook_deleted;
    total_skipped += hook_kept;

    eprintln!("[gc] Total: {} deleted, {} kept", total_deleted, total_skipped);

    Ok(GcResult {
        deleted: total_deleted,
        skipped: total_skipped,
    })
}

/// Walk the project to find all markdown documents and compute their hashes.
fn collect_document_hashes(root: &Path) -> Result<HashSet<String>> {
    let mut hashes = HashSet::new();

    // Walk project for .md files (skip .agent-doc/ itself, node_modules, target, .git)
    walk_for_docs(root, &mut hashes)?;

    Ok(hashes)
}

fn walk_for_docs(dir: &Path, hashes: &mut HashSet<String>) -> Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()), // Skip unreadable dirs
    };

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip hidden dirs and common large dirs
        if name_str.starts_with('.') || name_str == "node_modules" || name_str == "target" || name_str == "bin" {
            continue;
        }

        if path.is_dir() {
            walk_for_docs(&path, hashes)?;
        } else if path.extension().is_some_and(|e| e == "md") && let Ok(hash) = snapshot::doc_hash(&path) {
            hashes.insert(hash);
        }
    }

    Ok(())
}

/// Remove files from `dir` whose hash prefix doesn't match any known document.
fn clean_orphaned_files(
    dir: &Path,
    extensions: &[&str],
    known_hashes: &HashSet<String>,
    dry_run: bool,
) -> Result<(usize, usize)> {
    let mut deleted = 0;
    let mut skipped = 0;

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let file_name = path.file_name().unwrap_or_default().to_string_lossy();

        // Extract hash from filename (e.g., "abc123.md" -> "abc123")
        let hash = extensions.iter()
            .find_map(|ext| file_name.strip_suffix(&format!(".{}", ext)))
            .unwrap_or(&file_name);

        if known_hashes.contains(hash) {
            skipped += 1;
        } else {
            if dry_run {
                eprintln!("[gc] would delete: {}", path.display());
            } else {
                let _ = std::fs::remove_file(&path);
            }
            deleted += 1;
        }
    }

    Ok((deleted, skipped))
}

/// Clean stale lock files (>1 hour old) from locks/ and crdt/*.lock.
fn clean_stale_locks(agent_doc_dir: &Path, dry_run: bool) -> Result<(usize, usize)> {
    let stale_threshold = Duration::from_secs(3600);
    let mut deleted = 0;
    let mut kept = 0;

    for dir_name in &["locks", "crdt"] {
        let dir = agent_doc_dir.join(dir_name);
        if !dir.is_dir() {
            continue;
        }

        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let is_lock = path.extension().is_some_and(|e| e == "lock");
            if !is_lock {
                continue;
            }

            let is_stale = std::fs::metadata(&path)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.elapsed().ok())
                .map(|age| age > stale_threshold)
                .unwrap_or(false);

            if is_stale {
                if dry_run {
                    eprintln!("[gc] would delete stale lock: {}", path.display());
                } else {
                    let _ = std::fs::remove_file(&path);
                }
                deleted += 1;
            } else {
                kept += 1;
            }
        }
    }

    Ok((deleted, kept))
}

/// Clean old hook event files (>24 hours old) from hooks/post_write/ and hooks/post_commit/.
fn clean_old_hooks(agent_doc_dir: &Path, dry_run: bool) -> Result<(usize, usize)> {
    let max_age = Duration::from_secs(86400); // 24 hours
    let mut deleted = 0;
    let mut kept = 0;

    for sub in &["hooks/post_write", "hooks/post_commit"] {
        let dir = agent_doc_dir.join(sub);
        if !dir.is_dir() {
            continue;
        }

        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let is_old = std::fs::metadata(&path)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.elapsed().ok())
                .map(|age| age > max_age)
                .unwrap_or(false);

            if is_old {
                if dry_run {
                    eprintln!("[gc] would delete old hook: {}", path.display());
                } else {
                    let _ = std::fs::remove_file(&path);
                }
                deleted += 1;
            } else {
                kept += 1;
            }
        }
    }

    Ok((deleted, kept))
}

fn find_project_root_from_cwd() -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("failed to get CWD")?;
    let mut dir = cwd.as_path();
    loop {
        if dir.join(".agent-doc").is_dir() {
            return Ok(dir.to_path_buf());
        }
        match dir.parent() {
            Some(p) => dir = p,
            None => anyhow::bail!("no .agent-doc/ directory found (walked up from CWD)"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn gc_removes_orphaned_snapshots() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        // Create .agent-doc structure
        std::fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        std::fs::create_dir_all(root.join(".agent-doc/locks")).unwrap();

        // Create a document and its snapshot
        let doc = root.join("test.md");
        std::fs::write(&doc, "# Test\n").unwrap();
        let hash = snapshot::doc_hash(&doc).unwrap();
        std::fs::write(
            root.join(format!(".agent-doc/snapshots/{}.md", hash)),
            "snapshot",
        ).unwrap();

        // Create an orphaned snapshot (no matching document)
        std::fs::write(
            root.join(".agent-doc/snapshots/orphaned_hash_abc123.md"),
            "orphan",
        ).unwrap();

        let result = run(Some(root), false).unwrap();
        assert!(result.deleted >= 1, "should delete orphaned snapshot");
        assert!(result.skipped >= 1, "should keep valid snapshot");
    }
}
