//! Sync process-effect adapters.
//!
//! This crate owns lock-file acquisition, Linux `/proc` inspection, and sync
//! status writeback. Pure sync decisions remain in `agent-doc-sync`.

use agent_doc_sync::{SYNC_LOCK_POLL_INTERVAL, SyncLockProcess, is_stale_orphaned_sync_lock_owner};
use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub enum SyncLockAcquire {
    Acquired(File),
    Contended,
    Unavailable,
}

impl SyncLockAcquire {
    pub fn is_acquired(&self) -> bool {
        if let Self::Acquired(file) = self {
            let _ = file;
            true
        } else {
            false
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DeadPaneDetectedEventFacts<'a> {
    pub pane_id: &'a str,
    pub dead_status: Option<&'a str>,
    pub cycle_phase: Option<&'a str>,
    pub observed_window: Option<&'a str>,
    pub capture_path: Option<&'a Path>,
    pub last_visible_excerpt: Option<&'a str>,
}

pub fn persist_dead_pane_capture(
    file: &Path,
    session_id: &str,
    pane_id: &str,
    tail: &str,
) -> Option<PathBuf> {
    if tail.trim().is_empty() {
        return None;
    }
    let canonical = file
        .canonicalize()
        .ok()
        .unwrap_or_else(|| file.to_path_buf());
    let root = agent_doc_project_root_io::project_root_containing(&canonical)?;
    let dir = root.join(".agent-doc/logs/dead-panes");
    std::fs::create_dir_all(&dir).ok()?;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let pane_token = pane_id.trim_start_matches('%');
    let path = dir.join(format!("{session_id}-{timestamp}-pane-{pane_token}.log"));
    std::fs::write(&path, tail).ok()?;
    Some(path)
}

pub fn dead_pane_detected_event(facts: DeadPaneDetectedEventFacts<'_>) -> String {
    let mut event = format!(
        "pane_death_detected pane={} status={} cycle_phase={}",
        facts.pane_id,
        facts.dead_status.unwrap_or("unknown"),
        facts.cycle_phase.unwrap_or("none")
    );
    if let Some(window_id) = facts.observed_window {
        event.push_str(&format!(" window={window_id}"));
    }
    if let Some(path) = facts.capture_path {
        event.push_str(&format!(" capture={}", path.display()));
    }
    if let Some(excerpt) = facts.last_visible_excerpt {
        event.push_str(&format!(" last_visible_excerpt={excerpt}"));
    }
    event
}

pub fn dead_pane_cleanup_event(pane_id: &str) -> String {
    format!("pane_death_cleanup pane={pane_id} action=keep_dead policy=normal_sync_never_kills")
}

pub fn append_sync_log(msg: &str) {
    append_sync_log_at(Path::new("/tmp/agent-doc-sync.log"), msg);
}

pub fn append_sync_log_at(log_path: &Path, msg: &str) {
    use std::io::Write;
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(log_path) {
        let timestamp = agent_doc_log_time::current_log_timestamp();
        let _ = writeln!(file, "[{}] {}", timestamp, msg);
    }
}

pub fn acquire_sync_lock(
    lock_path: &Path,
    wait_budget: Duration,
    mut log: impl FnMut(String),
) -> SyncLockAcquire {
    if let Some(parent) = lock_path.parent()
        && let Err(err) = std::fs::create_dir_all(parent)
    {
        log(format!(
            "sync lock unavailable - failed to create {}: {}",
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
            log(format!(
                "sync lock unavailable - failed to open {}: {}",
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
                if !stale_owner_cleanup_attempted
                    && reap_stale_orphaned_sync_lock_owners(lock_path, &mut log)
                {
                    stale_owner_cleanup_attempted = true;
                    continue;
                }
                log(format!(
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

pub fn write_sync_status_with(
    file: &Path,
    text: &str,
    mut save_snapshot: impl FnMut(&Path, &str) -> Result<()>,
) -> Result<bool> {
    let doc = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {} for sync status update", file.display()))?;
    let components = agent_doc_element::element::parse(&doc)
        .with_context(|| format!("failed to parse components in {}", file.display()))?;
    let Some(status) = components
        .iter()
        .find(|comp| comp.name.as_str() == "status")
        .cloned()
    else {
        return Ok(false);
    };
    if status.content(&doc).trim() == text.trim() {
        return Ok(false);
    }

    let payload = if text.is_empty() {
        String::new()
    } else {
        format!("{text}\n")
    };
    let updated = status.replace_content(&doc, &payload);
    std::fs::write(file, &updated)
        .with_context(|| format!("failed to write {} for sync status update", file.display()))?;
    save_snapshot(file, &updated).with_context(|| {
        format!(
            "failed to update snapshot for {} after sync status update",
            file.display()
        )
    })?;
    Ok(true)
}

pub fn surface_frontmatter_status_with(
    file: &Path,
    phase: &str,
    err: &anyhow::Error,
    mut save_snapshot: impl FnMut(&Path, &str) -> Result<()>,
    mut log: impl FnMut(String),
) {
    let text = agent_doc_sync::sync_frontmatter_status_message(phase, err);
    match write_sync_status_with(file, &text, &mut save_snapshot) {
        Ok(true) => log(format!(
            "[sync] status: surfaced malformed frontmatter warning for {}",
            file.display()
        )),
        Ok(false) => {}
        Err(status_err) => log(format!(
            "[sync] warning: failed to surface malformed frontmatter status for {}: {}",
            file.display(),
            status_err
        )),
    }
}

pub fn clear_frontmatter_status_with(
    file: &Path,
    mut save_snapshot: impl FnMut(&Path, &str) -> Result<()>,
    mut log: impl FnMut(String),
) {
    let doc = match std::fs::read_to_string(file) {
        Ok(doc) => doc,
        Err(_) => return,
    };
    let components = match agent_doc_element::element::parse(&doc) {
        Ok(components) => components,
        Err(_) => return,
    };
    let Some(status) = components
        .iter()
        .find(|comp| comp.name.as_str() == "status")
        .cloned()
    else {
        return;
    };
    if !status
        .content(&doc)
        .trim_start()
        .starts_with(agent_doc_sync::SYNC_FRONTMATTER_STATUS_PREFIX)
    {
        return;
    }

    match write_sync_status_with(file, "", &mut save_snapshot) {
        Ok(true) => log(format!(
            "[sync] status: cleared malformed frontmatter warning for {}",
            file.display()
        )),
        Ok(false) => {}
        Err(status_err) => log(format!(
            "[sync] warning: failed to clear malformed frontmatter status for {}: {}",
            file.display(),
            status_err
        )),
    }
}

#[cfg(target_os = "linux")]
pub fn reap_stale_orphaned_sync_lock_owners(lock_path: &Path, mut log: impl FnMut(String)) -> bool {
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
            log(format!(
                "[sync] stale_sync_lock_owner_reaped pid={} age_ms={} cmd={}",
                process.pid,
                process.age.as_millis(),
                process.cmdline.join(" ")
            ));
        }
    }

    reaped_any
}

#[cfg(not(target_os = "linux"))]
pub fn reap_stale_orphaned_sync_lock_owners(_lock_path: &Path, _log: impl FnMut(String)) -> bool {
    false
}

#[cfg(target_os = "linux")]
pub fn sync_lock_process_from_proc(pid: u32, lock_path: &Path) -> SyncLockProcess {
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
pub fn read_proc_ppid(proc_dir: &Path) -> Option<u32> {
    let status = std::fs::read_to_string(proc_dir.join("status")).ok()?;
    status.lines().find_map(|line| {
        let value = line.strip_prefix("PPid:")?.trim();
        value.parse::<u32>().ok()
    })
}

#[cfg(target_os = "linux")]
pub fn read_proc_cmdline(proc_dir: &Path) -> Vec<String> {
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
pub fn read_proc_age(proc_dir: &Path) -> Option<Duration> {
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
pub fn proc_has_fd_for_path(proc_dir: &Path, target: &Path) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn persist_dead_pane_capture_writes_under_project_logs() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "body").unwrap();

        let path = persist_dead_pane_capture(&doc, "session-a", "%42", "tail\n")
            .expect("non-empty tail should persist");

        assert!(path.starts_with(dir.path().join(".agent-doc/logs/dead-panes")));
        assert!(path.to_string_lossy().contains("session-a-"));
        assert!(path.to_string_lossy().contains("-pane-42.log"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), "tail\n");
    }

    #[test]
    fn persist_dead_pane_capture_skips_empty_tail() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "body").unwrap();

        assert!(persist_dead_pane_capture(&doc, "session-a", "%42", "  \n").is_none());
    }

    #[test]
    fn dead_pane_events_keep_stable_fields() {
        let capture_path = PathBuf::from("/tmp/dead.log");
        let event = dead_pane_detected_event(DeadPaneDetectedEventFacts {
            pane_id: "%42",
            dead_status: Some("exited"),
            cycle_phase: Some("preflight_started"),
            observed_window: Some("@7"),
            capture_path: Some(&capture_path),
            last_visible_excerpt: Some("last_line"),
        });

        assert_eq!(
            event,
            "pane_death_detected pane=%42 status=exited cycle_phase=preflight_started window=@7 capture=/tmp/dead.log last_visible_excerpt=last_line"
        );
        assert_eq!(
            dead_pane_cleanup_event("%42"),
            "pane_death_cleanup pane=%42 action=keep_dead policy=normal_sync_never_kills"
        );
    }

    #[test]
    fn append_sync_log_writes_timestamped_entry() {
        let dir = tempfile::TempDir::new().unwrap();
        let log_path = dir.path().join("sync.log");
        let marker = format!("sync_log_test_marker_{}", std::process::id());

        append_sync_log_at(&log_path, &marker);

        let log_content = std::fs::read_to_string(&log_path).unwrap();
        let matching_line = log_content
            .lines()
            .find(|line| line.contains(&marker))
            .expect("marker line should exist");
        assert!(matching_line.starts_with('['), "{matching_line}");
        assert!(
            matching_line.contains("] sync_log_test_marker_"),
            "{matching_line}"
        );
    }

    #[test]
    fn acquire_sync_lock_times_out_when_lock_is_held() {
        let tmp = tempfile::TempDir::new().unwrap();
        let lock_path = tmp.path().join(".agent-doc/sync.lock");
        std::fs::create_dir_all(lock_path.parent().unwrap()).unwrap();

        let holder = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .unwrap();
        fs2::FileExt::lock_exclusive(&holder).unwrap();

        let start = Instant::now();
        let acquired = acquire_sync_lock(&lock_path, Duration::from_millis(120), |_| {});
        let elapsed = start.elapsed();

        fs2::FileExt::unlock(&holder).unwrap();
        assert!(
            matches!(acquired, SyncLockAcquire::Contended),
            "contended sync lock should time out instead of blocking"
        );
        assert!(
            elapsed < Duration::from_secs(1),
            "sync lock timeout should be bounded, elapsed={elapsed:?}"
        );
    }

    #[test]
    fn write_sync_status_updates_status_component_and_snapshot() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = tmp.path().join("doc.md");
        std::fs::write(&doc, "<!-- agent:status -->\nold\n<!-- /agent:status -->\n").unwrap();
        let mut snapshot = String::new();

        let changed = write_sync_status_with(&doc, "new status", |_, updated| {
            snapshot = updated.to_string();
            Ok(())
        })
        .unwrap();

        assert!(changed);
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("new status\n"));
        assert_eq!(snapshot, updated);
    }
}
