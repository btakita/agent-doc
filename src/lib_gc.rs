use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

fn platform_lib_name() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "libagent_doc.so"
    }
    #[cfg(target_os = "macos")]
    {
        "libagent_doc.dylib"
    }
    #[cfg(target_os = "windows")]
    {
        "agent_doc.dll"
    }
}

fn is_versioned_lib(name: &str) -> bool {
    #[cfg(target_os = "linux")]
    {
        name.starts_with("libagent_doc-") && name.ends_with(".so")
    }
    #[cfg(target_os = "macos")]
    {
        name.starts_with("libagent_doc-") && name.ends_with(".dylib")
    }
    #[cfg(target_os = "windows")]
    {
        name.starts_with("agent_doc-") && name.ends_with(".dll")
    }
}

fn is_pid_lock(name: &str, lib_name: &str) -> Option<u32> {
    let prefix = format!("{}.pid.", lib_name);
    if name.starts_with(&prefix) {
        name[prefix.len()..].parse::<u32>().ok()
    } else {
        None
    }
}

fn is_pid_alive(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        Path::new(&format!("/proc/{}", pid)).exists()
    }
    #[cfg(not(target_os = "linux"))]
    {
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
}

pub fn resolve_lib_dir() -> Result<PathBuf> {
    let exe = std::env::current_exe()?;
    exe.parent()
        .context("cannot determine binary directory")
        .map(|p| p.to_path_buf())
}

pub fn gc_libs(lib_dir: &Path) -> Result<GcResult> {
    let symlink_path = lib_dir.join(platform_lib_name());
    let current_target = if symlink_path.is_symlink() {
        std::fs::read_link(&symlink_path).ok()
    } else {
        None
    };

    let mut result = GcResult::default();

    let entries: Vec<_> = std::fs::read_dir(lib_dir)
        .with_context(|| format!("read dir {}", lib_dir.display()))?
        .filter_map(|e| e.ok())
        .collect();

    for entry in &entries {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if !is_versioned_lib(&name_str) {
            continue;
        }

        if let Some(ref target) = current_target
            && target.file_name() == Some(&name)
        {
            result.kept_current = Some(name_str.to_string());
            continue;
        }

        let so_path = entry.path();
        let mut live_pids = Vec::new();
        let mut dead_locks = Vec::new();

        for lock_entry in &entries {
            let lock_name = lock_entry.file_name();
            let lock_str = lock_name.to_string_lossy();
            if let Some(pid) = is_pid_lock(&lock_str, &name_str) {
                if is_pid_alive(pid) {
                    live_pids.push(pid);
                } else {
                    dead_locks.push(lock_entry.path());
                }
            }
        }

        for lock in &dead_locks {
            std::fs::remove_file(lock).ok();
            result.locks_removed += 1;
        }

        if live_pids.is_empty() {
            std::fs::remove_file(&so_path).ok();
            result.libs_removed.push(name_str.to_string());
        } else {
            result.kept_locked.push((name_str.to_string(), live_pids));
        }
    }

    Ok(result)
}

#[derive(Default)]
pub struct GcResult {
    pub kept_current: Option<String>,
    pub kept_locked: Vec<(String, Vec<u32>)>,
    pub libs_removed: Vec<String>,
    pub locks_removed: usize,
}

pub fn run(target_dir: Option<&str>) -> Result<()> {
    let lib_dir = match target_dir {
        Some(d) => PathBuf::from(d),
        None => resolve_lib_dir()?,
    };

    let result = gc_libs(&lib_dir)?;

    if let Some(ref current) = result.kept_current {
        eprintln!("[gc-libs] kept current: {}", current);
    }
    for (name, pids) in &result.kept_locked {
        eprintln!("[gc-libs] kept (live PIDs {:?}): {}", pids, name);
    }
    for name in &result.libs_removed {
        eprintln!("[gc-libs] removed: {}", name);
    }
    if result.locks_removed > 0 {
        eprintln!("[gc-libs] cleaned {} stale lock(s)", result.locks_removed);
    }
    if result.libs_removed.is_empty() && result.locks_removed == 0 {
        eprintln!("[gc-libs] nothing to clean");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn write_versioned(dir: &Path, version: &str) -> PathBuf {
        let name = crate::lib_install::versioned_lib_name(version);
        let path = dir.join(&name);
        fs::write(&path, format!("lib v{}", version)).unwrap();
        path
    }

    fn create_symlink(dir: &Path, target_name: &str) {
        let symlink = dir.join(platform_lib_name());
        #[cfg(unix)]
        std::os::unix::fs::symlink(target_name, &symlink).unwrap();
    }

    fn write_pid_lock(so_path: &Path, pid: u32) -> PathBuf {
        let lock = so_path.with_extension(format!(
            "{}.pid.{}",
            so_path.extension().unwrap().to_str().unwrap(),
            pid
        ));
        // Fix: lock name should be <so_name>.pid.<pid>
        let so_name = so_path.file_name().unwrap().to_str().unwrap();
        let lock = so_path
            .parent()
            .unwrap()
            .join(format!("{}.pid.{}", so_name, pid));
        fs::write(&lock, "").unwrap();
        lock
    }

    #[test]
    fn gc_removes_stale_versioned_lib() {
        let tmp = setup_dir();
        let v1 = write_versioned(tmp.path(), "1.0.0");
        write_versioned(tmp.path(), "2.0.0");
        let v2_name = crate::lib_install::versioned_lib_name("2.0.0");
        create_symlink(tmp.path(), &v2_name);

        let result = gc_libs(tmp.path()).unwrap();

        assert!(!v1.exists());
        assert_eq!(result.libs_removed.len(), 1);
        assert!(result.libs_removed[0].contains("1.0.0"));
        assert!(result.kept_current.unwrap().contains("2.0.0"));
    }

    #[test]
    fn gc_preserves_current_symlink_target() {
        let tmp = setup_dir();
        write_versioned(tmp.path(), "1.0.0");
        let v1_name = crate::lib_install::versioned_lib_name("1.0.0");
        create_symlink(tmp.path(), &v1_name);

        let result = gc_libs(tmp.path()).unwrap();

        assert!(tmp.path().join(&v1_name).exists());
        assert!(result.libs_removed.is_empty());
        assert!(result.kept_current.unwrap().contains("1.0.0"));
    }

    #[test]
    fn gc_preserves_lib_with_live_pid() {
        let tmp = setup_dir();
        let v1 = write_versioned(tmp.path(), "1.0.0");
        write_versioned(tmp.path(), "2.0.0");
        let v2_name = crate::lib_install::versioned_lib_name("2.0.0");
        create_symlink(tmp.path(), &v2_name);

        // Lock with current process PID (always alive)
        let my_pid = std::process::id();
        write_pid_lock(&v1, my_pid);

        let result = gc_libs(tmp.path()).unwrap();

        assert!(v1.exists());
        assert!(result.libs_removed.is_empty());
        assert_eq!(result.kept_locked.len(), 1);
        assert!(result.kept_locked[0].1.contains(&my_pid));
    }

    #[test]
    fn gc_removes_lib_with_dead_pid_lock() {
        let tmp = setup_dir();
        let v1 = write_versioned(tmp.path(), "1.0.0");
        write_versioned(tmp.path(), "2.0.0");
        let v2_name = crate::lib_install::versioned_lib_name("2.0.0");
        create_symlink(tmp.path(), &v2_name);

        // Lock with a PID that's almost certainly dead
        let dead_pid = 99999u32;
        let lock = write_pid_lock(&v1, dead_pid);

        let result = gc_libs(tmp.path()).unwrap();

        assert!(!v1.exists());
        assert!(!lock.exists());
        assert_eq!(result.libs_removed.len(), 1);
        assert_eq!(result.locks_removed, 1);
    }

    #[test]
    fn gc_noop_when_no_versioned_libs() {
        let tmp = setup_dir();
        // Only the symlink target (not versioned pattern)
        fs::write(tmp.path().join(platform_lib_name()), "regular file").unwrap();

        let result = gc_libs(tmp.path()).unwrap();

        assert!(result.libs_removed.is_empty());
        assert!(result.kept_current.is_none());
    }
}
