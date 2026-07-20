//! Filesystem probes for local `agent-doc` install freshness.
//!
//! This module owns mtime and source-location policy. Callers decide how to
//! classify or report stale artifacts.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Unix mtime (seconds) of `path`, following symlinks. `None` when
/// missing/unreadable.
pub fn artifact_mtime_secs(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

pub fn newest_artifact_mtime(paths: &[PathBuf]) -> Option<u64> {
    paths
        .iter()
        .filter_map(|path| artifact_mtime_secs(path))
        .max()
}

/// `~/.cargo/bin` (honoring `CARGO_HOME`), or `None` when unresolvable.
pub fn cargo_bin_dir() -> Option<PathBuf> {
    if let Ok(cargo_home) = std::env::var("CARGO_HOME")
        && !cargo_home.is_empty()
    {
        return Some(PathBuf::from(cargo_home).join("bin"));
    }
    let home = std::env::var("HOME").ok().filter(|h| !h.is_empty())?;
    Some(PathBuf::from(home).join(".cargo/bin"))
}

/// Newest mtime among `<bin_dir>/libagent_doc-*.so` (the lib-installed cdylib
/// the JetBrains plugin hot-reloads). Version-globbed because the cdylib is
/// named after the `agent-doc` binary crate version, not this crate's.
pub fn installed_agent_doc_cdylib_mtime(bin_dir: &Path) -> Option<u64> {
    std::fs::read_dir(bin_dir)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            (name.starts_with("libagent_doc-") && name.ends_with(".so"))
                .then(|| artifact_mtime_secs(&entry.path()))
                .flatten()
        })
        .max()
}

/// Locate the `agent-doc` source repo relative to the document's git root: the
/// root itself (standalone checkout) or `<root>/src/agent-doc` (the dogfood
/// submodule layout), identified by a `Cargo.toml` declaring the binary crate.
pub fn locate_agent_doc_source_repo(doc_git_root: &Path) -> Option<PathBuf> {
    [
        doc_git_root.to_path_buf(),
        doc_git_root.join("src/agent-doc"),
    ]
    .into_iter()
    .find(|candidate| {
        std::fs::read_to_string(candidate.join("Cargo.toml"))
            .map(|toml| toml.lines().any(|l| l.trim() == "name = \"agent-doc\""))
            .unwrap_or(false)
    })
}

/// Newest mtime (unix secs) among a crate's source/build input files.
///
/// The walk is bounded to `.rs`/`.toml`/`.md` files and skips build, VCS, and
/// cache directories. Fail-open `None` when nothing is readable.
pub fn newest_crate_source_mtime_secs(crate_root: &Path) -> Option<u64> {
    fn walk(dir: &Path, newest: &mut u64) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if matches!(
                    name.as_ref(),
                    "target" | ".git" | ".agent-doc" | "node_modules" | ".tsift" | "build" | "dist"
                ) {
                    continue;
                }
                walk(&entry.path(), newest);
            } else if file_type.is_file() {
                let path = entry.path();
                let ext_ok = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| matches!(e, "rs" | "toml" | "md"))
                    .unwrap_or(false);
                if !ext_ok {
                    continue;
                }
                if let Some(secs) = artifact_mtime_secs(&path) {
                    *newest = (*newest).max(secs);
                }
            }
        }
    }

    let mut newest = 0u64;
    walk(crate_root, &mut newest);
    (newest > 0).then_some(newest)
}

pub fn agent_doc_install_artifacts(repo: &Path) -> Vec<(&'static str, Option<u64>)> {
    let bin_dir = cargo_bin_dir();
    let release_dir = repo.join("target/release");
    let local_install_dir = repo.join("target/local-install/release-local");
    vec![
        (
            "~/.cargo/bin/agent-doc",
            bin_dir
                .as_deref()
                .and_then(|d| artifact_mtime_secs(&d.join("agent-doc"))),
        ),
        (
            "~/.cargo/bin cdylib",
            bin_dir
                .as_deref()
                .and_then(installed_agent_doc_cdylib_mtime),
        ),
        (
            "built agent-doc",
            newest_artifact_mtime(&[
                release_dir.join("agent-doc"),
                local_install_dir.join("agent-doc"),
            ]),
        ),
        (
            "built cdylib",
            newest_artifact_mtime(&[
                release_dir.join("libagent_doc.so"),
                local_install_dir.join("libagent_doc.so"),
            ]),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;
    use tempfile::TempDir;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvRestore {
        cargo_home: Option<String>,
        home: Option<String>,
    }

    impl EnvRestore {
        fn capture() -> Self {
            Self {
                cargo_home: std::env::var("CARGO_HOME").ok(),
                home: std::env::var("HOME").ok(),
            }
        }
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            unsafe {
                match &self.cargo_home {
                    Some(value) => std::env::set_var("CARGO_HOME", value),
                    None => std::env::remove_var("CARGO_HOME"),
                }
                match &self.home {
                    Some(value) => std::env::set_var("HOME", value),
                    None => std::env::remove_var("HOME"),
                }
            }
        }
    }

    fn set_mtime(path: &Path, secs: i64) {
        filetime::set_file_mtime(path, filetime::FileTime::from_unix_time(secs, 0)).unwrap();
    }

    #[test]
    fn newest_artifact_mtime_uses_freshest_existing_path() {
        let dir = TempDir::new().unwrap();
        let old = dir.path().join("target/release/agent-doc");
        let fresh = dir
            .path()
            .join("target/local-install/release-local/agent-doc");
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        std::fs::create_dir_all(fresh.parent().unwrap()).unwrap();
        std::fs::write(&old, "old").unwrap();
        std::fs::write(&fresh, "fresh").unwrap();
        set_mtime(&old, 1_000);
        set_mtime(&fresh, 2_000);

        assert_eq!(newest_artifact_mtime(&[old, fresh]), Some(2_000));
    }

    #[test]
    fn locate_agent_doc_source_repo_matches_root_and_dogfood_layout() {
        let agent_doc_manifest = "[package]\nname = \"agent-doc\"\nversion = \"0.0.0\"\n";

        let root = TempDir::new().unwrap();
        std::fs::write(root.path().join("Cargo.toml"), agent_doc_manifest).unwrap();
        assert_eq!(
            locate_agent_doc_source_repo(root.path()).as_deref(),
            Some(root.path())
        );

        let superproject = TempDir::new().unwrap();
        let src = superproject.path().join("src/agent-doc");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("Cargo.toml"), agent_doc_manifest).unwrap();
        assert_eq!(locate_agent_doc_source_repo(superproject.path()), Some(src));

        let other = TempDir::new().unwrap();
        std::fs::write(
            other.path().join("Cargo.toml"),
            "[package]\nname = \"something-else\"\n",
        )
        .unwrap();
        assert!(locate_agent_doc_source_repo(other.path()).is_none());
    }

    #[test]
    fn cargo_bin_dir_honors_cargo_home() {
        let _guard = ENV_LOCK.lock();
        let _restore = EnvRestore::capture();
        unsafe {
            std::env::set_var("CARGO_HOME", "/tmp/agent-doc-cargo");
            std::env::set_var("HOME", "/tmp/home");
        }

        assert_eq!(
            cargo_bin_dir(),
            Some(PathBuf::from("/tmp/agent-doc-cargo/bin"))
        );
    }

    #[test]
    fn cargo_bin_dir_falls_back_to_home() {
        let _guard = ENV_LOCK.lock();
        let _restore = EnvRestore::capture();
        unsafe {
            std::env::remove_var("CARGO_HOME");
            std::env::set_var("HOME", "/tmp/home");
        }

        assert_eq!(cargo_bin_dir(), Some(PathBuf::from("/tmp/home/.cargo/bin")));
    }

    #[test]
    fn installed_agent_doc_cdylib_mtime_uses_newest_versioned_so() {
        let dir = TempDir::new().unwrap();
        let old = dir.path().join("libagent_doc-0.1.0.so");
        let fresh = dir.path().join("libagent_doc-0.2.0.so");
        let ignored = dir.path().join("libagent_doc.so");
        std::fs::write(&old, "old").unwrap();
        std::fs::write(&fresh, "fresh").unwrap();
        std::fs::write(&ignored, "ignored").unwrap();
        set_mtime(&old, 1_000);
        set_mtime(&fresh, 2_000);
        set_mtime(&ignored, 3_000);

        assert_eq!(installed_agent_doc_cdylib_mtime(dir.path()), Some(2_000));
    }

    #[test]
    fn newest_crate_source_mtime_secs_skips_build_cache_and_dist_dirs() {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("src/lib.rs");
        let ignored_target = dir.path().join("target/newer.rs");
        let ignored_dist = dir.path().join("dist/newer.toml");
        let ignored_extension = dir.path().join("src/newer.txt");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::create_dir_all(ignored_target.parent().unwrap()).unwrap();
        std::fs::create_dir_all(ignored_dist.parent().unwrap()).unwrap();
        std::fs::write(&source, "source").unwrap();
        std::fs::write(&ignored_target, "target").unwrap();
        std::fs::write(&ignored_dist, "dist").unwrap();
        std::fs::write(&ignored_extension, "txt").unwrap();
        set_mtime(&source, 1_000);
        set_mtime(&ignored_target, 4_000);
        set_mtime(&ignored_dist, 3_000);
        set_mtime(&ignored_extension, 2_000);

        assert_eq!(newest_crate_source_mtime_secs(dir.path()), Some(1_000));
    }
}
