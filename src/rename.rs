//! # Module: rename
//!
//! Migrate session state after a document file rename/move.
//!
//! When a document is renamed, all hash-keyed state files (snapshots, locks,
//! pending, CRDT, pre-response) become orphaned because the hash is derived
//! from the canonical path. This module migrates those files from the old hash
//! to the new hash and updates the sessions registry.
//!
//! ## Spec
//!
//! - `run(old_path, new_path)` migrates all `.agent-doc/` state keyed by the
//!   old path's hash to the new path's hash. Updates `sessions.json` entries
//!   whose `file` field matches the old path.
//! - The old path may no longer exist on disk (rename already happened). In
//!   that case, `doc_hash_from_str` is used with the absolute path string
//!   instead of `doc_hash` (which requires `canonicalize`).
//! - Limitation: if the old path contained symlinks, the computed hash may not
//!   match the original because `canonicalize` resolves symlinks but our
//!   fallback does not.
//!
//! ## Agentic Contracts
//!
//! - All state file types are migrated: snapshots, locks, pending, crdt,
//!   pre-response.
//! - Missing source files are silently skipped (idempotent).
//! - Existing destination files cause an error (prevents accidental overwrite).
//! - Registry updates are performed under `RegistryLock`.

use anyhow::{Context, Result};
use std::path::Path;

use crate::{sessions, snapshot};

/// State file types to migrate, with their subdirectory and extension.
const STATE_FILES: &[(&str, &str)] = &[
    ("snapshots", "md"),
    ("baselines", "md"),
    ("locks", "lock"),
    ("pending", "md"),
    ("crdt", "yrs"),
    ("pre-response", "md"),
];

/// Migrate session state after a document rename.
pub fn run(old_path: &Path, new_path: &Path) -> Result<()> {
    // new_path must exist
    if !new_path.exists() {
        anyhow::bail!("new path does not exist: {}", new_path.display());
    }

    // Compute old hash
    let old_hash = if old_path.exists() {
        snapshot::doc_hash(old_path)?
    } else {
        // Old path no longer exists — resolve to absolute without canonicalize
        let abs = if old_path.is_absolute() {
            old_path.to_string_lossy().to_string()
        } else {
            let cwd = std::env::current_dir().context("failed to get current directory")?;
            cwd.join(old_path).to_string_lossy().to_string()
        };
        snapshot::doc_hash_from_str(&abs)
    };

    // Compute new hash (file exists, canonicalize works)
    let new_hash = snapshot::doc_hash(new_path)?;

    if old_hash == new_hash {
        eprintln!("[rename] hashes match — nothing to migrate");
        return Ok(());
    }

    // Find project root from the new path
    let canonical_new = new_path.canonicalize()?;
    let project_root = snapshot::find_project_root(&canonical_new)
        .context("no .agent-doc/ directory found above new path")?;

    // Migrate each state file type
    let mut migrated = 0u32;
    for &(subdir, ext) in STATE_FILES {
        let dir = project_root.join(".agent-doc").join(subdir);
        let old_file = dir.join(format!("{}.{}", old_hash, ext));
        let new_file = dir.join(format!("{}.{}", new_hash, ext));

        if !old_file.exists() {
            continue;
        }
        if new_file.exists() {
            anyhow::bail!(
                "destination already exists: {} (would overwrite)",
                new_file.display()
            );
        }
        std::fs::rename(&old_file, &new_file).with_context(|| {
            format!(
                "failed to rename {} → {}",
                old_file.display(),
                new_file.display()
            )
        })?;
        eprintln!(
            "[rename] {}/{}.{} → {}.{}",
            subdir,
            &old_hash[..8],
            ext,
            &new_hash[..8],
            ext
        );
        migrated += 1;
    }

    // Also migrate lock files with .md.lock suffix (snapshot locks)
    let locks_dir = project_root.join(".agent-doc/locks");
    let old_snap_lock = locks_dir.join(format!("{}.md.lock", old_hash));
    let new_snap_lock = locks_dir.join(format!("{}.md.lock", new_hash));
    if old_snap_lock.exists() && !new_snap_lock.exists() {
        std::fs::rename(&old_snap_lock, &new_snap_lock)?;
        migrated += 1;
    }

    // CRDT lock files
    let crdt_dir = project_root.join(".agent-doc/crdt");
    let old_crdt_lock = crdt_dir.join(format!("{}.yrs.lock", old_hash));
    let new_crdt_lock = crdt_dir.join(format!("{}.yrs.lock", new_hash));
    if old_crdt_lock.exists() && !new_crdt_lock.exists() {
        std::fs::rename(&old_crdt_lock, &new_crdt_lock)?;
        migrated += 1;
    }

    // Update sessions registry
    let old_path_str = old_path.to_string_lossy().to_string();
    let new_path_str = new_path.to_string_lossy().to_string();
    // Also compare against absolute forms
    let old_abs = if old_path.is_absolute() {
        old_path_str.clone()
    } else {
        let cwd = std::env::current_dir().unwrap_or_default();
        cwd.join(old_path).to_string_lossy().to_string()
    };
    let new_rel = new_path_str.clone();

    let _lock = sessions::RegistryLock::acquire(&sessions::registry_path())?;
    let mut registry = sessions::load()?;
    let mut updated_sessions = 0u32;
    for (_sid, entry) in registry.iter_mut() {
        if entry.file == old_path_str || entry.file == old_abs {
            entry.file = new_rel.clone();
            updated_sessions += 1;
        }
    }
    if updated_sessions > 0 {
        sessions::save(&registry)?;
    }

    eprintln!(
        "[rename] migrated {} state file(s), updated {} session(s): {} → {}",
        migrated,
        updated_sessions,
        old_path.display(),
        new_path.display()
    );
    Ok(())
}
