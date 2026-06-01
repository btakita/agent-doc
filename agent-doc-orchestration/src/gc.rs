//! # Module: gc
//!
//! ## Spec
//! - Garbage-collects orphaned files in `.agent-doc/` directories.
//! - Scans snapshots, crdt, pre-response, locks, baselines, annotations, and
//!   durable response captures for hash-keyed files/directories.
//! - Cross-references hashes against existing documents in the project.
//! - Removes files whose corresponding document no longer exists.
//! - Scans `.agent-doc/supervisor/*.sock` for orphaned sockets: checks
//!   sessions.json PID liveness, then tries socket connect as fallback.
//!   Removes dead sockets and prunes stale sessions.json entries.
//! - Removes stale `.agent-doc/typing/` indicator files (>7 days).
//! - Removes stale `.agent-doc/status/` files (>24 hours).
//! - Removes old `.agent-doc/repair-blocked/` diagnostic files (>7 days).
//! - Removes old `.agent-doc/codex-hooks/blocked-stop/` diagnostic files (>7 days).
//! - Closes stale `starting` actor records when no fresh supervisor lease proves
//!   that the actor is still booting.
//! - `--dry-run` flag shows what would be deleted without deleting.
//!
//! ## Agentic Contracts
//! - `run(root, dry_run)` — scans `.agent-doc/` under `root`, removes orphaned files.
//!   Returns `Ok(GcResult)` with counts of deleted/skipped files.
//! - Never deletes files or capture ledgers for documents that still exist on disk.
//! - Stale lock files (>1 hour old) are always cleaned regardless of document existence.
//! - `clean_orphaned_sockets` acquires `RegistryLock` when pruning sessions.json
//!   entries; read-only callers do not hold the lock.

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::sessions::{self, RegistryLock};
use crate::snapshot;
use crate::supervisor::ipc;

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
        anyhow::bail!(
            ".agent-doc/ directory not found in {}",
            project_root.display()
        );
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

    let captures_dir = agent_doc_dir.join("captures");
    if captures_dir.is_dir() {
        let (deleted, skipped) =
            clean_orphaned_capture_dirs(&captures_dir, &known_hashes, dry_run)?;
        if deleted > 0 || skipped > 0 {
            eprintln!("[gc] captures: {} deleted, {} kept", deleted, skipped);
        }
        total_deleted += deleted;
        total_skipped += skipped;
    }

    // Clean stale lock files (locks/ and crdt/*.lock)
    let (lock_deleted, lock_kept) = clean_stale_locks(&agent_doc_dir, dry_run)?;
    if lock_deleted > 0 {
        eprintln!(
            "[gc] locks: {} stale deleted, {} kept",
            lock_deleted, lock_kept
        );
    }
    total_deleted += lock_deleted;
    total_skipped += lock_kept;

    // Clean hook event files (hooks/post_write/, hooks/post_commit/)
    let (hook_deleted, hook_kept) = clean_old_hooks(&agent_doc_dir, dry_run)?;
    if hook_deleted > 0 {
        eprintln!(
            "[gc] hooks: {} old events deleted, {} kept",
            hook_deleted, hook_kept
        );
    }
    total_deleted += hook_deleted;
    total_skipped += hook_kept;

    // Clean stale typing indicators (>7 days)
    let (typing_deleted, typing_kept) = clean_stale_ephemeral_files(
        &agent_doc_dir.join("typing"),
        Duration::from_secs(7 * 86400),
        dry_run,
    )?;
    if typing_deleted > 0 {
        eprintln!(
            "[gc] typing: {} stale deleted, {} kept",
            typing_deleted, typing_kept
        );
    }
    total_deleted += typing_deleted;
    total_skipped += typing_kept;

    // Clean stale status files (>24 hours)
    let (status_deleted, status_kept) = clean_stale_ephemeral_files(
        &agent_doc_dir.join("status"),
        Duration::from_secs(86400),
        dry_run,
    )?;
    if status_deleted > 0 {
        eprintln!(
            "[gc] status: {} stale deleted, {} kept",
            status_deleted, status_kept
        );
    }
    total_deleted += status_deleted;
    total_skipped += status_kept;

    // Clean old repair-blocked diagnostics (>7 days)
    let (repair_deleted, repair_kept) = clean_stale_ephemeral_files(
        &agent_doc_dir.join("repair-blocked"),
        Duration::from_secs(7 * 86400),
        dry_run,
    )?;
    if repair_deleted > 0 {
        eprintln!(
            "[gc] repair-blocked: {} old diagnostics deleted, {} kept",
            repair_deleted, repair_kept
        );
    }
    total_deleted += repair_deleted;
    total_skipped += repair_kept;

    // Clean old Codex blocked-stop diagnostics (>7 days)
    let (blocked_stop_deleted, blocked_stop_kept) = clean_stale_ephemeral_files(
        &agent_doc_dir.join("codex-hooks/blocked-stop"),
        Duration::from_secs(7 * 86400),
        dry_run,
    )?;
    if blocked_stop_deleted > 0 {
        eprintln!(
            "[gc] codex blocked-stop: {} old diagnostics deleted, {} kept",
            blocked_stop_deleted, blocked_stop_kept
        );
    }
    total_deleted += blocked_stop_deleted;
    total_skipped += blocked_stop_kept;

    // Close stale starting actor projections (>1 hour, no fresh supervisor lease)
    let (actors_closed, actors_kept) = crate::project_controller::close_stale_starting_actors(
        &project_root,
        Duration::from_secs(3600),
        dry_run,
    )?;
    if actors_closed > 0 {
        eprintln!(
            "[gc] actors: {} stale starting closed, {} still active",
            actors_closed, actors_kept
        );
    }
    total_deleted += actors_closed;
    total_skipped += actors_kept;

    // Clean orphaned supervisor sockets + stale sessions.json entries
    let (sock_deleted, sock_kept) = clean_orphaned_sockets(&project_root, dry_run)?;
    if sock_deleted > 0 {
        eprintln!(
            "[gc] sockets: {} dead removed, {} alive kept",
            sock_deleted, sock_kept
        );
    }
    total_deleted += sock_deleted;
    total_skipped += sock_kept;

    eprintln!(
        "[gc] Total: {} deleted, {} kept",
        total_deleted, total_skipped
    );

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
        if name_str.starts_with('.')
            || name_str == "node_modules"
            || name_str == "target"
            || name_str == "bin"
        {
            continue;
        }

        if path.is_dir() {
            walk_for_docs(&path, hashes)?;
        } else if path.extension().is_some_and(|e| e == "md")
            && let Ok(hash) = snapshot::doc_hash(&path)
        {
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
        let hash = extensions
            .iter()
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

fn clean_orphaned_capture_dirs(
    dir: &Path,
    known_hashes: &HashSet<String>,
    dry_run: bool,
) -> Result<(usize, usize)> {
    let mut deleted = 0;
    let mut kept = 0;

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let hash = entry.file_name().to_string_lossy().to_string();
        if known_hashes.contains(&hash) {
            kept += 1;
            continue;
        }

        if dry_run {
            eprintln!("[gc] would delete capture ledger: {}", path.display());
        } else {
            let _ = std::fs::remove_dir_all(&path);
        }
        deleted += 1;
    }

    Ok((deleted, kept))
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

/// Clean stale files from a flat directory based on mtime age.
/// Used for typing indicators, status files, and repair-blocked diagnostics.
fn clean_stale_ephemeral_files(
    dir: &Path,
    max_age: Duration,
    dry_run: bool,
) -> Result<(usize, usize)> {
    if !dir.is_dir() {
        return Ok((0, 0));
    }

    let mut deleted = 0;
    let mut kept = 0;

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let is_stale = std::fs::metadata(&path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.elapsed().ok())
            .map(|age| age > max_age)
            .unwrap_or(false);

        if is_stale {
            if dry_run {
                eprintln!("[gc] would delete: {}", path.display());
            } else {
                let _ = std::fs::remove_file(&path);
            }
            deleted += 1;
        } else {
            kept += 1;
        }
    }

    Ok((deleted, kept))
}

/// Check if a process with the given PID is alive.
fn pid_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

/// Clean orphaned supervisor sockets and stale sessions.json entries.
///
/// For each `.sock` file in `.agent-doc/supervisor/`:
/// 1. Extract session UUID from filename
/// 2. Look up PID in sessions.json — if PID is alive, keep socket
/// 3. If PID is dead or no sessions.json entry, try connecting to socket
/// 4. If connect fails: remove socket file + prune sessions.json entry
///
/// Also prunes sessions.json entries whose PID is dead and socket is gone.
fn clean_orphaned_sockets(project_root: &Path, dry_run: bool) -> Result<(usize, usize)> {
    let supervisor_dir = project_root.join(".agent-doc/supervisor");
    if !supervisor_dir.is_dir() {
        return Ok((0, 0));
    }

    let registry = sessions::load_in(project_root).unwrap_or_default();
    let mut deleted = 0;
    let mut kept = 0;
    let mut keys_to_prune: Vec<String> = Vec::new();

    // Phase 1: scan socket files
    for entry in std::fs::read_dir(&supervisor_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "sock") {
            continue;
        }
        let file_name = path.file_name().unwrap_or_default().to_string_lossy();
        let uuid = match file_name.strip_suffix(".sock") {
            Some(u) => u.to_string(),
            None => continue,
        };

        // Check if the session has a live PID in the registry
        let registry_key = registry
            .iter()
            .find_map(|(key, entry)| (entry.session_id == uuid).then(|| key.clone()));
        let pid_is_alive = registry_key
            .as_deref()
            .and_then(|key| registry.get(key))
            .map(|entry| pid_alive(entry.pid))
            .unwrap_or(false);

        if pid_is_alive {
            kept += 1;
            continue;
        }

        // PID is dead or not in registry — try connecting as fallback
        let socket_responds = ipc::is_active(project_root, &uuid);
        if socket_responds {
            kept += 1;
            continue;
        }

        // Socket is dead — remove it
        if dry_run {
            eprintln!("[gc] would delete orphaned socket: {}", path.display());
        } else {
            let _ = std::fs::remove_file(&path);
        }
        deleted += 1;

        // Mark for sessions.json pruning if entry exists
        if let Some(key) = registry_key {
            keys_to_prune.push(key);
        }
    }

    // Phase 2: prune sessions.json entries for dead sockets
    // Also find entries whose PID is dead and socket doesn't exist
    for (registry_key, entry) in &registry {
        if keys_to_prune.contains(registry_key) {
            continue; // already marked
        }
        let sock_path = ipc::socket_path(project_root, &entry.session_id);
        if !sock_path.exists() && !pid_alive(entry.pid) {
            keys_to_prune.push(registry_key.clone());
            deleted += 1;
            if dry_run {
                eprintln!(
                    "[gc] would prune stale session entry: {} (pid {} dead, no socket)",
                    entry.session_id, entry.pid
                );
            }
        }
    }

    // Apply registry pruning
    if !keys_to_prune.is_empty() && !dry_run {
        let registry_path = sessions::registry_path_in(project_root);
        if let Ok(_lock) = RegistryLock::acquire(&registry_path) {
            // Reload under lock to avoid stale reads
            if let Ok(mut reg) = sessions::load_in(project_root) {
                for key in &keys_to_prune {
                    if let Some(entry) = reg.get(key) {
                        eprintln!("[gc] pruning stale session: {}", entry.session_id);
                    }
                    reg.remove(key);
                }
                let _ = sessions::save_in(project_root, &reg);
            }
        }
    }

    Ok((deleted, kept))
}

fn find_project_root_from_cwd() -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("failed to get CWD")?;
    crate::fs_util::find_project_root(&cwd)
        .context("no .agent-doc/ directory found (walked up from CWD)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::{SessionEntry, SessionRegistry};
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
        )
        .unwrap();

        // Create an orphaned snapshot (no matching document)
        std::fs::write(
            root.join(".agent-doc/snapshots/orphaned_hash_abc123.md"),
            "orphan",
        )
        .unwrap();

        let result = run(Some(root), false).unwrap();
        assert!(result.deleted >= 1, "should delete orphaned snapshot");
        assert!(result.skipped >= 1, "should keep valid snapshot");
    }

    fn make_session_entry(session_id: &str, pid: u32) -> SessionEntry {
        SessionEntry {
            pane: "%99999".to_string(),
            pid,
            cwd: "/tmp".to_string(),
            started: "2026-01-01T00:00:00Z".to_string(),
            session_id: session_id.to_string(),
            file: format!("{session_id}.md"),
            window: String::new(),
            supervisor_instance_id: String::new(),
        }
    }

    #[test]
    fn gc_removes_orphaned_socket_with_dead_pid() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let supervisor_dir = root.join(".agent-doc/supervisor");
        std::fs::create_dir_all(&supervisor_dir).unwrap();

        // Create a fake socket file for a dead session
        let uuid = "dead-session-uuid";
        std::fs::write(supervisor_dir.join(format!("{uuid}.sock")), "").unwrap();

        // Register in sessions.json with a dead PID
        let mut reg = SessionRegistry::new();
        reg.insert(uuid.to_string(), make_session_entry(uuid, 999999));
        sessions::save_in(root, &reg).unwrap();

        let (deleted, kept) = clean_orphaned_sockets(root, false).unwrap();
        assert_eq!(deleted, 1, "should remove orphaned socket");
        assert_eq!(kept, 0);

        // Socket file should be gone
        assert!(!supervisor_dir.join(format!("{uuid}.sock")).exists());

        // sessions.json entry should be pruned
        let loaded = sessions::load_in(root).unwrap();
        assert!(!loaded.values().any(|entry| entry.session_id == uuid));
    }

    #[test]
    fn gc_keeps_socket_with_live_pid() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let supervisor_dir = root.join(".agent-doc/supervisor");
        std::fs::create_dir_all(&supervisor_dir).unwrap();

        let uuid = "live-session-uuid";
        std::fs::write(supervisor_dir.join(format!("{uuid}.sock")), "").unwrap();

        // Register with current PID (alive)
        let mut reg = SessionRegistry::new();
        reg.insert(
            uuid.to_string(),
            make_session_entry(uuid, std::process::id()),
        );
        sessions::save_in(root, &reg).unwrap();

        let (deleted, kept) = clean_orphaned_sockets(root, false).unwrap();
        assert_eq!(deleted, 0);
        assert_eq!(kept, 1, "should keep socket with live PID");

        // Socket file should still exist
        assert!(supervisor_dir.join(format!("{uuid}.sock")).exists());
    }

    #[test]
    fn gc_removes_socket_without_session_entry() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let supervisor_dir = root.join(".agent-doc/supervisor");
        std::fs::create_dir_all(&supervisor_dir).unwrap();

        // Socket with no sessions.json entry and no live listener
        let uuid = "leaked-socket";
        std::fs::write(supervisor_dir.join(format!("{uuid}.sock")), "").unwrap();

        let (deleted, kept) = clean_orphaned_sockets(root, false).unwrap();
        assert_eq!(deleted, 1, "should remove leaked socket");
        assert_eq!(kept, 0);
        assert!(!supervisor_dir.join(format!("{uuid}.sock")).exists());
    }

    #[test]
    fn gc_prunes_session_entry_with_dead_pid_and_no_socket() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc/supervisor")).unwrap();

        // sessions.json entry with dead PID, no socket file
        let uuid = "ghost-session";
        let mut reg = SessionRegistry::new();
        reg.insert(uuid.to_string(), make_session_entry(uuid, 999999));
        sessions::save_in(root, &reg).unwrap();

        let (deleted, _kept) = clean_orphaned_sockets(root, false).unwrap();
        assert_eq!(deleted, 1, "should count pruned entry");

        let loaded = sessions::load_in(root).unwrap();
        assert!(!loaded.values().any(|entry| entry.session_id == uuid));
    }

    #[test]
    fn gc_dry_run_does_not_delete_sockets() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let supervisor_dir = root.join(".agent-doc/supervisor");
        std::fs::create_dir_all(&supervisor_dir).unwrap();

        let uuid = "dry-run-test";
        let sock_path = supervisor_dir.join(format!("{uuid}.sock"));
        std::fs::write(&sock_path, "").unwrap();

        let mut reg = SessionRegistry::new();
        reg.insert(uuid.to_string(), make_session_entry(uuid, 999999));
        sessions::save_in(root, &reg).unwrap();

        let (deleted, kept) = clean_orphaned_sockets(root, true).unwrap();
        assert_eq!(deleted, 1, "should report would-delete count");
        assert_eq!(kept, 0);

        // But file should still exist
        assert!(sock_path.exists());

        // And sessions.json should be unmodified
        let loaded = sessions::load_in(root).unwrap();
        assert!(loaded.values().any(|entry| entry.session_id == uuid));
    }

    #[test]
    fn gc_removes_stale_typing_indicators() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let typing_dir = root.join(".agent-doc/typing");
        std::fs::create_dir_all(&typing_dir).unwrap();

        // Create a stale indicator (mtime 8 days ago)
        let stale = typing_dir.join("abc123");
        std::fs::write(&stale, "1000000000000").unwrap();
        let old_time = std::time::SystemTime::now() - Duration::from_secs(8 * 86400);
        let _ = filetime::set_file_mtime(&stale, filetime::FileTime::from_system_time(old_time));

        // Create a fresh indicator
        let fresh = typing_dir.join("def456");
        std::fs::write(&fresh, "9999999999999").unwrap();

        let result = run(Some(root), false).unwrap();
        assert!(result.deleted >= 1);
        assert!(!stale.exists(), "stale typing indicator should be removed");
        assert!(fresh.exists(), "fresh typing indicator should be kept");
    }

    #[test]
    fn gc_removes_old_repair_blocked() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let rb_dir = root.join(".agent-doc/repair-blocked");
        std::fs::create_dir_all(&rb_dir).unwrap();

        let stale = rb_dir.join("abc123-1000000000.json");
        std::fs::write(&stale, r#"{"reason":"test"}"#).unwrap();
        let old_time = std::time::SystemTime::now() - Duration::from_secs(8 * 86400);
        let _ = filetime::set_file_mtime(&stale, filetime::FileTime::from_system_time(old_time));

        let fresh = rb_dir.join("def456-9999999999.json");
        std::fs::write(&fresh, r#"{"reason":"recent"}"#).unwrap();

        let result = run(Some(root), false).unwrap();
        assert!(result.deleted >= 1);
        assert!(!stale.exists(), "old repair-blocked should be removed");
        assert!(fresh.exists(), "recent repair-blocked should be kept");
    }

    #[test]
    fn gc_removes_old_codex_blocked_stop_diagnostics() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let blocked_dir = root.join(".agent-doc/codex-hooks/blocked-stop");
        std::fs::create_dir_all(&blocked_dir).unwrap();

        let stale = blocked_dir.join("abc123-1000000000.json");
        std::fs::write(&stale, r#"{"kind":"blocked_replay_payload"}"#).unwrap();
        let old_time = std::time::SystemTime::now() - Duration::from_secs(8 * 86400);
        let _ = filetime::set_file_mtime(&stale, filetime::FileTime::from_system_time(old_time));

        let fresh = blocked_dir.join("def456-9999999999.json");
        std::fs::write(&fresh, r#"{"kind":"blocked_replay_payload"}"#).unwrap();

        let result = run(Some(root), false).unwrap();
        assert!(result.deleted >= 1);
        assert!(
            !stale.exists(),
            "old blocked-stop diagnostic should be removed"
        );
        assert!(
            fresh.exists(),
            "recent blocked-stop diagnostic should be kept"
        );
    }

    #[test]
    fn gc_removes_stale_status_files() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let status_dir = root.join(".agent-doc/status");
        std::fs::create_dir_all(&status_dir).unwrap();

        let stale = status_dir.join("abc123");
        std::fs::write(&stale, "generating").unwrap();
        let old_time = std::time::SystemTime::now() - Duration::from_secs(2 * 86400);
        let _ = filetime::set_file_mtime(&stale, filetime::FileTime::from_system_time(old_time));

        let fresh = status_dir.join("def456");
        std::fs::write(&fresh, "writing").unwrap();

        let result = run(Some(root), false).unwrap();
        assert!(result.deleted >= 1);
        assert!(!stale.exists(), "stale status file should be removed");
        assert!(fresh.exists(), "fresh status file should be kept");
    }

    #[test]
    fn gc_dry_run_preserves_ephemeral_files() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let typing_dir = root.join(".agent-doc/typing");
        std::fs::create_dir_all(&typing_dir).unwrap();

        let stale = typing_dir.join("abc123");
        std::fs::write(&stale, "1000000000000").unwrap();
        let old_time = std::time::SystemTime::now() - Duration::from_secs(8 * 86400);
        let _ = filetime::set_file_mtime(&stale, filetime::FileTime::from_system_time(old_time));

        let result = run(Some(root), true).unwrap();
        assert!(result.deleted >= 1, "dry run reports would-delete count");
        assert!(stale.exists(), "dry run should not actually delete");
    }

    #[test]
    fn gc_no_supervisor_dir_returns_zero() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        // No supervisor/ dir

        let (deleted, kept) = clean_orphaned_sockets(root, false).unwrap();
        assert_eq!(deleted, 0);
        assert_eq!(kept, 0);
    }

    #[test]
    fn gc_mixed_live_and_dead_sockets() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let supervisor_dir = root.join(".agent-doc/supervisor");
        std::fs::create_dir_all(&supervisor_dir).unwrap();

        let live_uuid = "live-session";
        let dead_uuid = "dead-session";

        std::fs::write(supervisor_dir.join(format!("{live_uuid}.sock")), "").unwrap();
        std::fs::write(supervisor_dir.join(format!("{dead_uuid}.sock")), "").unwrap();

        let mut reg = SessionRegistry::new();
        reg.insert(
            live_uuid.to_string(),
            make_session_entry(live_uuid, std::process::id()),
        );
        reg.insert(dead_uuid.to_string(), make_session_entry(dead_uuid, 999999));
        sessions::save_in(root, &reg).unwrap();

        let (deleted, kept) = clean_orphaned_sockets(root, false).unwrap();
        assert_eq!(deleted, 1, "dead socket removed");
        assert_eq!(kept, 1, "live socket kept");

        assert!(supervisor_dir.join(format!("{live_uuid}.sock")).exists());
        assert!(!supervisor_dir.join(format!("{dead_uuid}.sock")).exists());

        let loaded = sessions::load_in(root).unwrap();
        assert!(loaded.values().any(|entry| entry.session_id == live_uuid));
        assert!(!loaded.values().any(|entry| entry.session_id == dead_uuid));
    }
}
