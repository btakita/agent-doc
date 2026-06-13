use super::*;

#[derive(Debug)]
pub(crate) enum SyncLockAcquire {
    Acquired(File),
    Contended,
    Unavailable,
}

impl SyncLockAcquire {
    pub(crate) fn is_acquired(&self) -> bool {
        if let Self::Acquired(file) = self {
            let _ = file;
            true
        } else {
            false
        }
    }
}

pub(crate) fn acquire_sync_lock(lock_path: &Path, wait_budget: Duration) -> SyncLockAcquire {
    if let Some(parent) = lock_path.parent()
        && let Err(err) = std::fs::create_dir_all(parent)
    {
        sync_log(&format!(
            "sync lock unavailable — failed to create {}: {}",
            parent.display(),
            err
        ));
        return SyncLockAcquire::Unavailable;
    }

    let file = match OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(lock_path)
    {
        Ok(file) => file,
        Err(err) => {
            sync_log(&format!(
                "sync lock unavailable — failed to open {}: {}",
                lock_path.display(),
                err
            ));
            return SyncLockAcquire::Unavailable;
        }
    };

    let started = Instant::now();
    let mut stale_owner_cleanup_attempted = false;
    loop {
        use fs2::FileExt;
        match file.try_lock_exclusive() {
            Ok(()) => return SyncLockAcquire::Acquired(file),
            Err(err) if started.elapsed() >= wait_budget => {
                if !stale_owner_cleanup_attempted && reap_stale_orphaned_sync_lock_owners(lock_path)
                {
                    stale_owner_cleanup_attempted = true;
                    continue;
                }
                sync_log(&format!(
                    "sync lock contention exceeded {}ms at {}: {}",
                    wait_budget.as_millis(),
                    lock_path.display(),
                    err
                ));
                return SyncLockAcquire::Contended;
            }
            Err(_) => std::thread::sleep(SYNC_LOCK_POLL_INTERVAL),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SyncLockProcess {
    pub(crate) pid: u32,
    pub(crate) ppid: u32,
    pub(crate) age: Duration,
    pub(crate) cmdline: Vec<String>,
    pub(crate) has_lock_fd: bool,
}

pub(crate) fn is_stale_orphaned_sync_lock_owner(process: &SyncLockProcess) -> bool {
    process.ppid == 1
        && process.age >= STALE_SYNC_LOCK_OWNER_AGE
        && process.has_lock_fd
        && process
            .cmdline
            .first()
            .is_some_and(|bin| bin.ends_with("agent-doc"))
        && process.cmdline.iter().any(|arg| arg == "sync")
}

#[cfg(target_os = "linux")]
pub(crate) fn reap_stale_orphaned_sync_lock_owners(lock_path: &Path) -> bool {
    let lock_path = lock_path
        .canonicalize()
        .unwrap_or_else(|_| lock_path.to_path_buf());
    let current_pid = std::process::id();
    let mut reaped_any = false;

    let Ok(entries) = std::fs::read_dir("/proc") else {
        return false;
    };

    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == current_pid {
            continue;
        }

        let process = sync_lock_process_from_proc(pid, &lock_path);
        if !is_stale_orphaned_sync_lock_owner(&process) {
            continue;
        }

        let rc = unsafe { libc::kill(process.pid as libc::pid_t, libc::SIGTERM) };
        if rc == 0 {
            reaped_any = true;
            let message = format!(
                "[sync] stale_sync_lock_owner_reaped pid={} age_ms={} cmd={}",
                process.pid,
                process.age.as_millis(),
                process.cmdline.join(" ")
            );
            eprintln!("{}", message);
            sync_log(&message);
        }
    }

    reaped_any
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn reap_stale_orphaned_sync_lock_owners(_lock_path: &Path) -> bool {
    false
}

#[cfg(target_os = "linux")]
pub(crate) fn sync_lock_process_from_proc(pid: u32, lock_path: &Path) -> SyncLockProcess {
    let proc_dir = PathBuf::from("/proc").join(pid.to_string());
    let ppid = read_proc_ppid(&proc_dir).unwrap_or(0);
    let age = read_proc_age(&proc_dir).unwrap_or(Duration::ZERO);
    let cmdline = read_proc_cmdline(&proc_dir);
    let has_lock_fd = proc_has_fd_for_path(&proc_dir, lock_path);

    SyncLockProcess {
        pid,
        ppid,
        age,
        cmdline,
        has_lock_fd,
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn read_proc_ppid(proc_dir: &Path) -> Option<u32> {
    let status = std::fs::read_to_string(proc_dir.join("status")).ok()?;
    status.lines().find_map(|line| {
        let value = line.strip_prefix("PPid:")?.trim();
        value.parse::<u32>().ok()
    })
}

#[cfg(target_os = "linux")]
pub(crate) fn read_proc_cmdline(proc_dir: &Path) -> Vec<String> {
    std::fs::read(proc_dir.join("cmdline"))
        .ok()
        .map(|bytes| {
            bytes
                .split(|byte| *byte == 0)
                .filter(|part| !part.is_empty())
                .filter_map(|part| String::from_utf8(part.to_vec()).ok())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(target_os = "linux")]
pub(crate) fn read_proc_age(proc_dir: &Path) -> Option<Duration> {
    let stat = std::fs::read_to_string(proc_dir.join("stat")).ok()?;
    let after_comm = stat.rsplit_once(") ")?.1;
    let fields: Vec<&str> = after_comm.split_whitespace().collect();
    let start_ticks = fields.get(19)?.parse::<u64>().ok()?;
    let uptime_secs = std::fs::read_to_string("/proc/uptime")
        .ok()?
        .split_whitespace()
        .next()?
        .parse::<f64>()
        .ok()?;
    let ticks_per_second = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if ticks_per_second <= 0 {
        return None;
    }
    let start_secs = start_ticks as f64 / ticks_per_second as f64;
    if uptime_secs < start_secs {
        return Some(Duration::ZERO);
    }
    Some(Duration::from_secs_f64(uptime_secs - start_secs))
}

#[cfg(target_os = "linux")]
pub(crate) fn proc_has_fd_for_path(proc_dir: &Path, target: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(proc_dir.join("fd")) else {
        return false;
    };
    entries.flatten().any(|entry| {
        std::fs::read_link(entry.path())
            .ok()
            .map(|path| path == target)
            .unwrap_or(false)
    })
}
