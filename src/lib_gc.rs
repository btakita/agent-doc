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
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        let ret = unsafe { libc::kill(pid as libc::pid_t, 0) };
        ret == 0
            || std::io::Error::last_os_error()
                .raw_os_error()
                .is_some_and(|code| code == libc::EPERM)
    }
    #[cfg(windows)]
    {
        windows_pid_alive(pid)
    }
    #[cfg(not(any(unix, windows)))]
    {
        true
    }
}

#[cfg(windows)]
fn windows_pid_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }

        let mut exit_code = 0;
        let ok = GetExitCodeProcess(handle, &mut exit_code) != 0;
        CloseHandle(handle);
        ok && exit_code == STILL_ACTIVE as u32
    }
}

#[cfg(test)]
#[cfg(windows)]
mod windows_tests {
    use super::*;

    #[test]
    fn windows_pid_alive_detects_current_process() {
        assert!(windows_pid_alive(std::process::id()));
    }

    #[test]
    fn windows_pid_alive_rejects_missing_process() {
        assert!(!windows_pid_alive(u32::MAX));
    }
}

pub fn resolve_lib_dir() -> Result<PathBuf> {
    let exe = std::env::current_exe()?;
    exe.parent()
        .context("cannot determine binary directory")
        .map(|p| p.to_path_buf())
}

/// Whether a sweep deletes, or only reports what it would delete.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GcMode {
    Apply,
    /// `#gclibsdryrun` (GH #58): the first use of a cleanup command should not
    /// be its first destructive one. A plan run reports the same decisions
    /// without touching the directory.
    DryRun,
}

impl GcMode {
    fn deletes(self) -> bool {
        self == Self::Apply
    }
}

pub fn gc_libs(lib_dir: &Path) -> Result<GcResult> {
    gc_libs_with_pid_alive(lib_dir, is_pid_alive, GcMode::Apply)
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn plan_gc_libs(lib_dir: &Path) -> Result<GcResult> {
    gc_libs_with_pid_alive(lib_dir, is_pid_alive, GcMode::DryRun)
}

fn gc_libs_with_pid_alive(
    lib_dir: &Path,
    pid_alive: impl Fn(u32) -> bool,
    mode: GcMode,
) -> Result<GcResult> {
    let symlink_path = lib_dir.join(platform_lib_name());
    let current_target = if symlink_path.is_symlink() {
        std::fs::read_link(&symlink_path).ok()
    } else {
        None
    };
    // GH #132: a release-tarball install (`agent-doc upgrade` before the fix)
    // wrote the canonical library as a PLAIN FILE. Comparing filenames against
    // a symlink target then matched nothing, so every versioned library —
    // including a byte-identical copy of the installed one — was classed "not
    // the installed library". Resolve the plain file by identity instead: the
    // same inode, or the same bytes.
    let plain_canonical =
        (current_target.is_none() && symlink_path.is_file()).then(|| symlink_path.clone());

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

        if let Some(ref canonical) = plain_canonical
            && same_library(canonical, &entry.path())
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
                if pid_alive(pid) {
                    live_pids.push(pid);
                } else {
                    dead_locks.push(lock_entry.path());
                }
            }
        }

        for lock in &dead_locks {
            if mode.deletes() {
                std::fs::remove_file(lock).ok();
            }
            result.locks_removed += 1;
        }

        if live_pids.is_empty() {
            if mode.deletes() {
                std::fs::remove_file(&so_path).ok();
            }
            result.libs_removed.push(name_str.to_string());
        } else {
            result.kept_locked.push((name_str.to_string(), live_pids));
        }
    }

    // GH #132: the per-library lock sweep above only visits versioned libraries
    // that are candidates for removal, so markers naming the canonical
    // (unversioned) path or the kept-current library were never swept, and
    // dead-pid `libagent_doc.so.pid.<pid>` markers survived every run.
    let mut lock_bases = vec![platform_lib_name().to_string()];
    lock_bases.extend(result.kept_current.clone());
    for entry in &entries {
        let lock_name = entry.file_name();
        let lock_str = lock_name.to_string_lossy();
        let dead = lock_bases
            .iter()
            .any(|base| is_pid_lock(&lock_str, base).is_some_and(|pid| !pid_alive(pid)));
        if dead {
            if mode.deletes() {
                std::fs::remove_file(entry.path()).ok();
            }
            result.locks_removed += 1;
        }
    }

    Ok(result)
}

/// Whether `candidate` is the same library as the plain-file `canonical`:
/// the same inode (a hard link), or byte-identical contents.
fn same_library(canonical: &Path, candidate: &Path) -> bool {
    let (Ok(a), Ok(b)) = (std::fs::metadata(canonical), std::fs::metadata(candidate)) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if a.dev() == b.dev() && a.ino() == b.ino() {
            return true;
        }
    }
    if a.len() != b.len() {
        return false;
    }
    match (std::fs::read(canonical), std::fs::read(candidate)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

#[derive(Default)]
pub struct GcResult {
    pub kept_current: Option<String>,
    pub kept_locked: Vec<(String, Vec<u32>)>,
    pub libs_removed: Vec<String>,
    pub locks_removed: usize,
}

impl GcResult {
    /// Why the entries in `libs_removed` are reapable, phrased for an operator
    /// reading a `--dry-run` plan.
    fn removal_reason(&self) -> String {
        match self.kept_current {
            Some(ref current) => format!("superseded by {current}, no live holder"),
            None => "not the installed library, no live holder".to_string(),
        }
    }
}

pub fn run(target_dir: Option<&str>, dry_run: bool) -> Result<()> {
    let lib_dir = match target_dir {
        Some(d) => PathBuf::from(d),
        None => resolve_lib_dir()?,
    };

    let mode = if dry_run {
        GcMode::DryRun
    } else {
        GcMode::Apply
    };
    let result = gc_libs_with_pid_alive(&lib_dir, is_pid_alive, mode)?;

    if let Some(ref current) = result.kept_current {
        eprintln!("[gc-libs] kept current: {}", current);
    }
    for (name, pids) in &result.kept_locked {
        eprintln!("[gc-libs] kept (live PIDs {:?}): {}", pids, name);
    }
    let verb = if dry_run { "would remove" } else { "removed" };
    for name in &result.libs_removed {
        eprintln!("[gc-libs] {verb}: {name} ({})", result.removal_reason());
    }
    if result.locks_removed > 0 {
        let lock_verb = if dry_run { "would clean" } else { "cleaned" };
        eprintln!(
            "[gc-libs] {lock_verb} {} stale lock(s) (naming a PID that is not alive)",
            result.locks_removed
        );
    }
    if result.libs_removed.is_empty() && result.locks_removed == 0 {
        eprintln!("[gc-libs] nothing to clean");
    }
    if dry_run && !(result.libs_removed.is_empty() && result.locks_removed == 0) {
        eprintln!("[gc-libs] dry run: nothing was deleted; re-run without --dry-run to apply");
    }

    Ok(())
}

/// One-line quiet sweep for the install path (`#gclibsoninstall`, GH #58).
///
/// The reaper had exactly one caller — the `gc-libs` subcommand — so nothing
/// reaped on install, on upgrade, on startup, or from a timer, and
/// `~/.cargo/bin` grew by one cdylib per `lib-install` forever. A failure here
/// is reported and swallowed: cleanup must never fail an install that already
/// succeeded.
///
/// `label` names the install path in the log prefix (`lib-install`, `upgrade`);
/// GH #132 added the release-upgrade caller. Each reaped library is named so an
/// operator can see exactly what an install reclaimed.
pub fn gc_libs_after_install(lib_dir: &Path, label: &str) -> Option<GcResult> {
    match gc_libs(lib_dir) {
        Ok(result) => {
            if result.libs_removed.is_empty() && result.locks_removed == 0 {
                return Some(result);
            }
            eprintln!(
                "[{label}] gc-libs reaped {} superseded librar{} and {} stale lock(s)",
                result.libs_removed.len(),
                if result.libs_removed.len() == 1 {
                    "y"
                } else {
                    "ies"
                },
                result.locks_removed
            );
            for name in &result.libs_removed {
                eprintln!(
                    "[{label}] gc-libs removed: {name} ({})",
                    result.removal_reason()
                );
            }
            for (name, pids) in &result.kept_locked {
                eprintln!("[{label}] gc-libs kept (live PIDs {pids:?}): {name}");
            }
            Some(result)
        }
        Err(err) => {
            eprintln!("[{label}] gc-libs skipped: {err:#}");
            None
        }
    }
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

        let my_pid = std::process::id();
        write_pid_lock(&v1, my_pid);

        let result =
            gc_libs_with_pid_alive(tmp.path(), |pid| pid == my_pid, GcMode::Apply).unwrap();

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

        // Deliberately use the live test process PID: the injected probe, not
        // ambient process-table timing or PID reuse, defines this fixture.
        let dead_pid = std::process::id();
        let lock = write_pid_lock(&v1, dead_pid);

        let result = gc_libs_with_pid_alive(tmp.path(), |_| false, GcMode::Apply).unwrap();

        assert!(!v1.exists());
        assert!(!lock.exists());
        assert_eq!(result.libs_removed.len(), 1);
        assert_eq!(result.locks_removed, 1);
    }

    /// A dry run must report exactly what an apply run would do, and touch
    /// nothing. Asserting only "reports something" would pass against a sweep
    /// that still deleted, so the files are checked too.
    #[test]
    fn dry_run_reports_the_same_plan_without_deleting_anything() {
        let tmp = setup_dir();
        let v1 = write_versioned(tmp.path(), "1.0.0");
        write_versioned(tmp.path(), "2.0.0");
        let v2_name = crate::lib_install::versioned_lib_name("2.0.0");
        create_symlink(tmp.path(), &v2_name);
        let dead_lock = write_pid_lock(&v1, std::process::id());

        let planned = gc_libs_with_pid_alive(tmp.path(), |_| false, GcMode::DryRun).unwrap();

        assert_eq!(planned.libs_removed.len(), 1);
        assert!(planned.libs_removed[0].contains("1.0.0"));
        assert_eq!(planned.locks_removed, 1);
        assert!(v1.exists(), "dry run deleted the library");
        assert!(dead_lock.exists(), "dry run deleted the lock");
        assert!(
            planned.removal_reason().contains("superseded by"),
            "{}",
            planned.removal_reason()
        );

        let applied = gc_libs_with_pid_alive(tmp.path(), |_| false, GcMode::Apply).unwrap();
        assert_eq!(applied.libs_removed, planned.libs_removed);
        assert_eq!(applied.locks_removed, planned.locks_removed);
        assert!(!v1.exists());
        assert!(!dead_lock.exists());
    }

    /// The install path is the reaper's new caller (GH #58). It must reap the
    /// predecessor and keep the just-installed symlink target.
    #[test]
    fn install_sweep_reaps_predecessors_and_keeps_the_new_current() {
        let tmp = setup_dir();
        let v1 = write_versioned(tmp.path(), "1.0.0");
        let v2 = write_versioned(tmp.path(), "2.0.0");
        let v2_name = crate::lib_install::versioned_lib_name("2.0.0");
        create_symlink(tmp.path(), &v2_name);

        gc_libs_after_install(tmp.path(), "lib-install");

        assert!(!v1.exists());
        assert!(v2.exists());
    }

    /// Without a symlink (a tarball install replaces the library in place) the
    /// sweep has no "current" to protect, so it must not invent one — but it
    /// must still leave the unversioned library itself alone.
    #[test]
    fn a_plain_file_install_keeps_the_unversioned_library_and_reaps_the_versioned_pile() {
        let tmp = setup_dir();
        let v1 = write_versioned(tmp.path(), "1.0.0");
        let plain = tmp.path().join(platform_lib_name());
        fs::write(&plain, "regular file").unwrap();

        let planned = plan_gc_libs(tmp.path()).unwrap();
        assert!(planned.kept_current.is_none());
        assert_eq!(planned.libs_removed.len(), 1);
        assert!(
            planned
                .removal_reason()
                .contains("not the installed library"),
            "{}",
            planned.removal_reason()
        );

        gc_libs(tmp.path()).unwrap();
        assert!(!v1.exists());
        assert!(plain.exists());
    }

    /// GH #132: with a plain-file canonical library, the byte-identical
    /// versioned copy IS the installed library and must be kept as current —
    /// never classed "not the installed library".
    #[test]
    fn a_plain_file_install_keeps_its_byte_identical_versioned_copy() {
        let tmp = setup_dir();
        let v1 = write_versioned(tmp.path(), "1.0.0");
        let v2 = write_versioned(tmp.path(), "2.0.0");
        let plain = tmp.path().join(platform_lib_name());
        fs::write(&plain, fs::read(&v2).unwrap()).unwrap();

        let result = gc_libs(tmp.path()).unwrap();

        assert!(v2.exists(), "the installed library was reaped");
        assert!(plain.exists());
        assert!(!v1.exists());
        assert!(result.kept_current.unwrap().contains("2.0.0"));
        assert_eq!(result.libs_removed.len(), 1);
        assert!(result.libs_removed[0].contains("1.0.0"));
    }

    #[cfg(unix)]
    #[test]
    fn a_plain_file_install_keeps_a_hard_linked_versioned_copy() {
        let tmp = setup_dir();
        let v2 = write_versioned(tmp.path(), "2.0.0");
        let plain = tmp.path().join(platform_lib_name());
        fs::hard_link(&v2, &plain).unwrap();

        let result = gc_libs(tmp.path()).unwrap();

        assert!(v2.exists());
        assert!(result.libs_removed.is_empty());
        assert!(result.kept_current.unwrap().contains("2.0.0"));
    }

    /// GH #132: dead-pid markers naming the canonical (unversioned) library or
    /// the kept-current library were never swept. Live ones must survive.
    #[test]
    fn gc_sweeps_dead_locks_on_canonical_and_current_libraries() {
        let tmp = setup_dir();
        let v2 = write_versioned(tmp.path(), "2.0.0");
        let v2_name = crate::lib_install::versioned_lib_name("2.0.0");
        create_symlink(tmp.path(), &v2_name);
        let canonical = tmp.path().join(platform_lib_name());
        let dead_canonical = tmp
            .path()
            .join(format!("{}.pid.{}", platform_lib_name(), 7523));
        fs::write(&dead_canonical, "").unwrap();
        let live_canonical = tmp
            .path()
            .join(format!("{}.pid.{}", platform_lib_name(), 14558));
        fs::write(&live_canonical, "").unwrap();
        let dead_current = write_pid_lock(&v2, 54003);

        let result = gc_libs_with_pid_alive(tmp.path(), |pid| pid == 14558, GcMode::Apply).unwrap();

        assert!(!dead_canonical.exists());
        assert!(!dead_current.exists());
        assert!(live_canonical.exists());
        assert!(canonical.exists());
        assert!(v2.exists());
        assert_eq!(result.locks_removed, 2);
    }

    /// The labeled install sweep is shared by `lib-install` and `upgrade`.
    #[test]
    fn install_sweep_reports_what_it_reaped() {
        let tmp = setup_dir();
        write_versioned(tmp.path(), "1.0.0");
        write_versioned(tmp.path(), "2.0.0");
        create_symlink(tmp.path(), &crate::lib_install::versioned_lib_name("2.0.0"));

        let result = gc_libs_after_install(tmp.path(), "upgrade").unwrap();

        assert_eq!(result.libs_removed.len(), 1);
        assert!(result.libs_removed[0].contains("1.0.0"));
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
