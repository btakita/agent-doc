//! # Module: retired_store
//!
//! ## Spec
//! - Lists and removes retired `.agent-doc/` stores: directories that an older
//!   agent-doc design wrote and that no current reader or writer touches.
//! - The only retired store today is `live-buffer/`, the full-buffer editor
//!   sidecar retired in agent-doc 0.34.160 when the Lazily/CP CRDT became the
//!   sole live editor authority. Nothing on the hot path reads, writes, or
//!   prunes it, so it otherwise grows forever.
//! - Three modes: `Report` (plain `agent-doc gc`) names the store and how to
//!   remove it; `DryRun` (`gc --retired-stores --dry-run`) lists the path, file
//!   count, bytes, and last write; `Remove` (`gc --retired-stores`) deletes it.
//!   Preflight auto-GC never calls this module: the operator decides when a
//!   retired store is deleted.
//!
//! ## Agentic Contracts
//! - Only `<agent_doc_dir>/<retired name>` is touched; sibling `.agent-doc/`
//!   entries are never read or removed.
//! - Symlinks are never followed. A symlink inside the store is unlinked (its
//!   target is untouched and is not counted); a store root that is itself a
//!   symlink, or not a directory, is refused and left in place.
//! - Removal is best-effort per entry: every entry that fails to delete is
//!   recorded in `failures` with its error, the walk continues, and the store
//!   root is removed only when it is empty.

use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// A `.agent-doc/` subdirectory left behind by a retired design.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetiredStore {
    /// Directory name directly under `.agent-doc/`.
    pub name: &'static str,
    /// Why it is safe to delete, shown in every listing.
    pub retired_because: &'static str,
}

/// Every retired store `agent-doc gc --retired-stores` may remove.
pub const RETIRED_STORES: &[RetiredStore] = &[RetiredStore {
    name: "live-buffer",
    retired_because: "editor live-buffer sidecar, retired in agent-doc 0.34.160; the Lazily/CP CRDT is the sole live editor authority and nothing reads or writes this store",
}];

/// What a retired-store pass does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetiredStoreMode {
    /// Name the store and the command that removes it. Never deletes.
    Report,
    /// List what `Remove` would delete. Never deletes.
    DryRun,
    /// Delete the store.
    Remove,
}

/// One retired store found under `.agent-doc/`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RetiredStoreReport {
    pub name: &'static str,
    pub path: PathBuf,
    /// Regular files and symlinks found (symlinks are counted, never followed).
    pub files: u64,
    /// Subdirectories found below the store root.
    pub dirs: u64,
    /// Symlinks found; each is unlinked without touching its target.
    pub symlinks: u64,
    /// Bytes held by regular files (symlink targets are not counted).
    pub bytes: u64,
    /// Newest modification time among regular files.
    pub newest_mtime: Option<SystemTime>,
    /// Entries (files, symlinks, directories, and the root) actually deleted.
    pub removed: u64,
    /// Entries that could not be read or deleted, with the error.
    pub failures: Vec<(PathBuf, String)>,
    /// Set when the store root was refused (symlink or not a directory).
    pub refused: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RetiredStoresResult {
    pub stores: Vec<RetiredStoreReport>,
}

impl RetiredStoresResult {
    /// Every entry that could not be removed, across stores.
    pub fn failure_count(&self) -> usize {
        self.stores
            .iter()
            .map(|store| store.failures.len() + usize::from(store.refused.is_some()))
            .sum()
    }
}

/// Resolve the project root (as `agent-doc gc` does) and run a retired-store
/// pass over its `.agent-doc/`.
pub fn run_retired_stores(
    root: Option<&Path>,
    mode: RetiredStoreMode,
) -> Result<RetiredStoresResult> {
    let project_root = match root {
        Some(root) => root.to_path_buf(),
        None => agent_doc_project_root_io::project_root_from_cwd()?,
    };
    let agent_doc_dir = project_root.join(".agent-doc");
    if !agent_doc_dir.is_dir() {
        anyhow::bail!(
            ".agent-doc/ directory not found in {}",
            project_root.display()
        );
    }
    let result = sweep_retired_stores(&agent_doc_dir, mode);
    if mode != RetiredStoreMode::Report && result.stores.is_empty() {
        eprintln!(
            "[gc] retired stores: none under {}",
            agent_doc_dir.display()
        );
    }
    Ok(result)
}

/// Run one pass over every [`RETIRED_STORES`] entry under `agent_doc_dir`,
/// printing one `[gc] retired store` line per store found plus one per failure.
pub fn sweep_retired_stores(agent_doc_dir: &Path, mode: RetiredStoreMode) -> RetiredStoresResult {
    let mut result = RetiredStoresResult::default();
    for store in RETIRED_STORES {
        let path = agent_doc_dir.join(store.name);
        let Ok(root_meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        let mut report = RetiredStoreReport {
            name: store.name,
            path: path.clone(),
            ..RetiredStoreReport::default()
        };
        if root_meta.file_type().is_symlink() {
            report.refused = Some("store root is a symlink; not following it".to_string());
        } else if !root_meta.is_dir() {
            report.refused = Some("store root is not a directory".to_string());
        }
        if let Some(reason) = &report.refused {
            eprintln!(
                "[gc] retired store {}: {} refused: {reason}; left in place",
                store.name,
                path.display()
            );
            result.stores.push(report);
            continue;
        }

        let remove = mode == RetiredStoreMode::Remove;
        walk_store(&path, &mut report, remove);
        if remove {
            match fs::remove_dir(&path) {
                Ok(()) => report.removed += 1,
                Err(error) => report.failures.push((path.clone(), error.to_string())),
            }
        }
        print_report(store, &report, mode);
        result.stores.push(report);
    }
    result
}

/// Depth-first walk that never follows a symlink. With `remove`, each entry is
/// deleted after it is counted, and a directory after its children.
fn walk_store(dir: &Path, report: &mut RetiredStoreReport, remove: bool) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            report.failures.push((dir.to_path_buf(), error.to_string()));
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                report.failures.push((dir.to_path_buf(), error.to_string()));
                continue;
            }
        };
        let path = entry.path();
        let meta = match fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(error) => {
                report.failures.push((path, error.to_string()));
                continue;
            }
        };
        let file_type = meta.file_type();
        if file_type.is_dir() {
            report.dirs += 1;
            walk_store(&path, report, remove);
            if remove {
                match fs::remove_dir(&path) {
                    Ok(()) => report.removed += 1,
                    Err(error) => report.failures.push((path, error.to_string())),
                }
            }
            continue;
        }
        report.files += 1;
        if file_type.is_symlink() {
            report.symlinks += 1;
        } else {
            report.bytes += meta.len();
            if let Ok(modified) = meta.modified() {
                report.newest_mtime = Some(
                    report
                        .newest_mtime
                        .map_or(modified, |newest| newest.max(modified)),
                );
            }
        }
        if remove {
            // `remove_file` on a symlink unlinks the link, never its target.
            match fs::remove_file(&path) {
                Ok(()) => report.removed += 1,
                Err(error) => report.failures.push((path, error.to_string())),
            }
        }
    }
}

fn print_report(store: &RetiredStore, report: &RetiredStoreReport, mode: RetiredStoreMode) {
    let summary = format!(
        "{} files{}, {} ({} bytes), last written {}",
        report.files,
        if report.symlinks > 0 {
            format!(" ({} symlinks, not followed)", report.symlinks)
        } else {
            String::new()
        },
        format_bytes(report.bytes),
        report.bytes,
        format_last_write(report.newest_mtime),
    );
    match mode {
        RetiredStoreMode::Report => eprintln!(
            "[gc] retired store {}: {} ({summary}) kept; list it with `agent-doc gc --retired-stores --dry-run`, remove it with `agent-doc gc --retired-stores`",
            store.name,
            report.path.display(),
        ),
        RetiredStoreMode::DryRun => eprintln!(
            "[gc] retired store {}: would remove {} ({summary}) [{}]",
            store.name,
            report.path.display(),
            store.retired_because,
        ),
        RetiredStoreMode::Remove => {
            for (path, error) in &report.failures {
                eprintln!(
                    "[gc] retired store {}: failed to remove {}: {error}",
                    store.name,
                    path.display()
                );
            }
            eprintln!(
                "[gc] retired store {}: {} {} ({summary}); {} entries removed, {} failed",
                store.name,
                if report.failures.is_empty() {
                    "removed"
                } else {
                    "partially removed"
                },
                report.path.display(),
                report.removed,
                report.failures.len(),
            );
        }
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = "B";
    for next in UNITS {
        if value < 1024.0 {
            break;
        }
        value /= 1024.0;
        unit = next;
    }
    format!("{value:.1} {unit}")
}

fn format_last_write(newest: Option<SystemTime>) -> String {
    let Some(newest) = newest else {
        return "never (no regular files)".to_string();
    };
    match newest.elapsed() {
        Ok(age) => format!("{} days ago", age.as_secs() / 86_400),
        Err(_) => "in the future".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// A project `.agent-doc/` holding a retired `live-buffer/` store (two files,
    /// one nested) next to live state that gc must never touch.
    fn fixture() -> (TempDir, PathBuf) {
        let tmp = TempDir::new().unwrap();
        let agent_doc = tmp.path().join(".agent-doc");
        let live_buffer = agent_doc.join("live-buffer");
        fs::create_dir_all(live_buffer.join("nested")).unwrap();
        fs::write(
            live_buffer.join("001f01473453546e.jetbrains-1-abc"),
            b"12345",
        )
        .unwrap();
        fs::write(live_buffer.join("nested/00e03c1ed796d7d9"), b"abc").unwrap();
        fs::create_dir_all(agent_doc.join("snapshots")).unwrap();
        fs::write(agent_doc.join("snapshots/keep.md"), b"snapshot").unwrap();
        fs::write(agent_doc.join("state.db"), b"db").unwrap();
        (tmp, agent_doc)
    }

    fn assert_live_state_untouched(agent_doc: &Path) {
        assert_eq!(
            fs::read(agent_doc.join("snapshots/keep.md")).unwrap(),
            b"snapshot"
        );
        assert_eq!(fs::read(agent_doc.join("state.db")).unwrap(), b"db");
    }

    #[test]
    fn dry_run_lists_the_retired_store_without_deleting() {
        let (_tmp, agent_doc) = fixture();
        for mode in [RetiredStoreMode::DryRun, RetiredStoreMode::Report] {
            let result = sweep_retired_stores(&agent_doc, mode);
            assert_eq!(result.stores.len(), 1, "{mode:?}");
            let store = &result.stores[0];
            assert_eq!(store.name, "live-buffer");
            assert_eq!(store.path, agent_doc.join("live-buffer"));
            assert_eq!(
                (store.files, store.dirs, store.bytes),
                (2, 1, 8),
                "{mode:?}"
            );
            assert!(store.newest_mtime.is_some());
            assert_eq!(store.removed, 0, "{mode:?} must not delete");
            assert!(store.failures.is_empty());
            assert!(
                agent_doc
                    .join("live-buffer/nested/00e03c1ed796d7d9")
                    .is_file()
            );
        }
        assert_live_state_untouched(&agent_doc);
    }

    #[test]
    fn remove_deletes_only_the_retired_store() {
        let (_tmp, agent_doc) = fixture();
        let result = sweep_retired_stores(&agent_doc, RetiredStoreMode::Remove);
        assert_eq!(result.failure_count(), 0, "{result:?}");
        let store = &result.stores[0];
        assert_eq!((store.files, store.bytes), (2, 8));
        // two files, one nested dir, and the store root
        assert_eq!(store.removed, 4);
        assert!(!agent_doc.join("live-buffer").exists());
        assert_live_state_untouched(&agent_doc);

        let again = sweep_retired_stores(&agent_doc, RetiredStoreMode::Remove);
        assert!(
            again.stores.is_empty(),
            "a removed store is not found again"
        );
    }

    #[cfg(unix)]
    #[test]
    fn remove_unlinks_symlinks_without_following_them() {
        let (tmp, agent_doc) = fixture();
        let outside = tmp.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("precious.md"), b"operator data").unwrap();
        let live_buffer = agent_doc.join("live-buffer");
        std::os::unix::fs::symlink(&outside, live_buffer.join("dir-link")).unwrap();
        std::os::unix::fs::symlink(outside.join("precious.md"), live_buffer.join("file-link"))
            .unwrap();

        let listed = sweep_retired_stores(&agent_doc, RetiredStoreMode::DryRun);
        let store = &listed.stores[0];
        assert_eq!(store.symlinks, 2);
        assert_eq!(store.bytes, 8, "symlink targets are never counted");

        let result = sweep_retired_stores(&agent_doc, RetiredStoreMode::Remove);
        assert_eq!(result.failure_count(), 0, "{result:?}");
        assert!(!live_buffer.exists());
        assert_eq!(
            fs::read(outside.join("precious.md")).unwrap(),
            b"operator data"
        );
        assert_live_state_untouched(&agent_doc);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_store_root_is_refused_and_left_in_place() {
        let tmp = TempDir::new().unwrap();
        let agent_doc = tmp.path().join(".agent-doc");
        let elsewhere = tmp.path().join("elsewhere");
        fs::create_dir_all(&agent_doc).unwrap();
        fs::create_dir_all(&elsewhere).unwrap();
        fs::write(elsewhere.join("precious.md"), b"operator data").unwrap();
        std::os::unix::fs::symlink(&elsewhere, agent_doc.join("live-buffer")).unwrap();

        let result = sweep_retired_stores(&agent_doc, RetiredStoreMode::Remove);
        assert_eq!(result.failure_count(), 1);
        let reason = result.stores[0].refused.as_deref().unwrap();
        assert!(reason.contains("symlink"), "{reason}");
        assert_eq!(result.stores[0].removed, 0);
        assert!(fs::symlink_metadata(agent_doc.join("live-buffer")).is_ok());
        assert_eq!(
            fs::read(elsewhere.join("precious.md")).unwrap(),
            b"operator data"
        );
    }

    #[cfg(unix)]
    #[test]
    fn partial_failures_are_reported_and_the_walk_continues() {
        use std::os::unix::fs::PermissionsExt;
        let (_tmp, agent_doc) = fixture();
        let nested = agent_doc.join("live-buffer/nested");
        // Read+execute but not writable: its file cannot be unlinked.
        fs::set_permissions(&nested, fs::Permissions::from_mode(0o555)).unwrap();
        let result = sweep_retired_stores(&agent_doc, RetiredStoreMode::Remove);
        fs::set_permissions(&nested, fs::Permissions::from_mode(0o755)).unwrap();
        if !nested.join("00e03c1ed796d7d9").exists() {
            // Running as root ignores directory write permission; nothing to prove.
            return;
        }
        assert!(
            result.failure_count() > 0,
            "an undeleted entry must be reported"
        );
        let store = &result.stores[0];
        let failed: Vec<&PathBuf> = store.failures.iter().map(|(path, _)| path).collect();
        assert!(
            failed.contains(&&nested.join("00e03c1ed796d7d9")),
            "{failed:?}"
        );
        assert!(
            !agent_doc
                .join("live-buffer/001f01473453546e.jetbrains-1-abc")
                .exists(),
            "the walk must continue past a failed entry"
        );
        assert!(
            agent_doc.join("live-buffer").is_dir(),
            "a non-empty root stays"
        );
        assert_live_state_untouched(&agent_doc);
    }

    #[test]
    fn an_absent_store_reports_nothing() {
        let tmp = TempDir::new().unwrap();
        let agent_doc = tmp.path().join(".agent-doc");
        fs::create_dir_all(&agent_doc).unwrap();
        for mode in [
            RetiredStoreMode::Report,
            RetiredStoreMode::DryRun,
            RetiredStoreMode::Remove,
        ] {
            assert!(sweep_retired_stores(&agent_doc, mode).stores.is_empty());
        }
    }

    #[test]
    fn format_bytes_uses_binary_units() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(2048), "2.0 KiB");
        assert_eq!(format_bytes(1_825_361_101), "1.7 GiB");
    }
}
