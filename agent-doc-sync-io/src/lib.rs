//! Sync process-effect adapters.
//!
//! This crate owns lock-file acquisition, Linux `/proc` inspection, and sync
//! status writeback. Pure sync decisions remain in `agent-doc-sync`.

use agent_doc_sync::{SYNC_LOCK_POLL_INTERVAL, SyncLockProcess, is_stale_orphaned_sync_lock_owner};
use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub mod resync;
pub mod sync;

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
    let effects = runtime_effects()?;
    let doc = effects
        .resolve_current_document(file, "sync_status_document")
        .with_context(|| {
            format!(
                "failed to resolve {} for sync status update",
                file.display()
            )
        })?;
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
    effects
        .write_current_document(file, &updated)
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
    let doc = match runtime_effects()
        .and_then(|effects| effects.resolve_current_document(file, "sync_status_document"))
    {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncSessionCheckStatus {
    Ok(String),
    Interrupted(String),
}

pub trait SyncRuntimeEffects: Send + Sync + 'static {
    fn resolve_current_document(&self, file: &Path, source: &str) -> Result<String>;

    fn write_current_document(&self, file: &Path, content: &str) -> Result<()>;

    fn commit(&self, file: &Path) -> Result<bool>;

    fn detect_jb_cache_conflict_cancel_recoverable(&self, file: &Path) -> Result<bool>;

    fn detect_uncommitted_closeout_drift(&self, file: &Path) -> Result<Option<String>>;

    fn repair(&self, file: &Path) -> Result<agent_doc_turn::repair::RepairOutcome>;

    fn repair_stale_preflight_started_cycle(
        &self,
        file: &Path,
    ) -> Result<agent_doc_turn::repair::RepairOutcome>;

    fn save_pending(&self, file: &Path, response: &str) -> Result<()>;

    fn session_check_inspect(&self, file: &Path) -> Result<SyncSessionCheckStatus>;

    /// Read the controller-owned actor projection for a document in another
    /// project root without focusing, resuming, or provisioning its pane.
    ///
    /// Exact-visible editor sync uses this observation to route an already
    /// owned pane even when document-content authority is temporarily
    /// unavailable. The foreign binding remains ephemeral input to the parent
    /// layout effect and must not be copied into its durable registry.
    fn resolve_cross_root_document_pane(
        &self,
        project_root: &Path,
        file: &Path,
    ) -> Result<Option<agent_doc_controller_io::project_controller::ControllerTmuxActorBinding>>;

    /// Ask the controller that owns `project_root` to ensure `file` has an actor
    /// pane, returning the controller-proven pane binding. This is an effect:
    /// callers must not copy the foreign binding into their durable registry.
    fn ensure_cross_root_document_pane(
        &self,
        project_root: &Path,
        file: &Path,
    ) -> Result<Option<String>>;

    #[allow(clippy::too_many_arguments)]
    fn provision_pane(
        &self,
        tmux: &tmux_router::Tmux,
        file: &Path,
        session_id: &str,
        file_path: &str,
        context_session: Option<&str>,
        col_args: &[String],
        route_after_start: bool,
    ) -> Result<String>;
}

static RUNTIME_EFFECTS: OnceLock<&'static dyn SyncRuntimeEffects> = OnceLock::new();

pub fn install_runtime_effects(effects: &'static dyn SyncRuntimeEffects) {
    let _ = RUNTIME_EFFECTS.set(effects);
}

pub(crate) fn runtime_effects() -> Result<&'static dyn SyncRuntimeEffects> {
    if let Some(effects) = RUNTIME_EFFECTS.get().copied() {
        return Ok(effects);
    }
    #[cfg(test)]
    {
        return Ok(&TEST_RUNTIME_EFFECTS);
    }
    #[allow(unreachable_code)]
    Err(anyhow::anyhow!(
        "agent-doc sync runtime effects were not installed"
    ))
}

#[cfg(test)]
struct TestSyncRuntimeEffects;

#[cfg(test)]
static TEST_RUNTIME_EFFECTS: TestSyncRuntimeEffects = TestSyncRuntimeEffects;

#[cfg(test)]
impl TestSyncRuntimeEffects {
    fn git_root_for(file: &Path) -> Option<PathBuf> {
        let dir = file.parent().unwrap_or_else(|| Path::new("."));
        let output = std::process::Command::new("git")
            .current_dir(dir)
            .args(["rev-parse", "--show-toplevel"])
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| PathBuf::from(String::from_utf8_lossy(&output.stdout).trim().to_string()))
    }

    fn commit_file(file: &Path) -> Result<bool> {
        let Some(root) = Self::git_root_for(file) else {
            return Ok(false);
        };
        let rel = file.strip_prefix(&root).unwrap_or(file);
        let add = std::process::Command::new("git")
            .current_dir(&root)
            .arg("add")
            .arg(rel)
            .output()?;
        if !add.status.success() {
            anyhow::bail!(
                "git add failed: {}",
                String::from_utf8_lossy(&add.stderr).trim()
            );
        }
        let diff = std::process::Command::new("git")
            .current_dir(&root)
            .args(["diff", "--cached", "--quiet", "--"])
            .arg(rel)
            .status()?;
        if diff.success() {
            return Ok(false);
        }
        let commit = std::process::Command::new("git")
            .current_dir(&root)
            .args([
                "commit",
                "-m",
                "agent-doc sync test repair",
                "--no-verify",
                "--",
            ])
            .arg(rel)
            .output()?;
        if !commit.status.success() {
            anyhow::bail!(
                "git commit failed: {}",
                String::from_utf8_lossy(&commit.stderr).trim()
            );
        }
        Ok(true)
    }

    fn exchange_patch_body(response: &str) -> Result<String> {
        if response.contains("<!-- patch:backlog -->") && response.contains("not-a-list") {
            anyhow::bail!("pending/backlog patch changed non-list content");
        }
        let start_marker = "<!-- patch:exchange -->";
        let end_marker = "<!-- /patch:exchange -->";
        let Some(start) = response.find(start_marker) else {
            anyhow::bail!("pending response did not contain an exchange patch");
        };
        let body_start = start + start_marker.len();
        let Some(end_rel) = response[body_start..].find(end_marker) else {
            anyhow::bail!("pending response exchange patch was not closed");
        };
        let body = response[body_start..body_start + end_rel]
            .trim_matches('\n')
            .to_string();
        if body.trim().is_empty() {
            anyhow::bail!("pending response exchange patch was empty");
        }
        Ok(body)
    }

    fn apply_exchange_patch(file: &Path, response: &str) -> Result<bool> {
        let body = Self::exchange_patch_body(response)?;
        let doc = std::fs::read_to_string(file)?;
        if doc.contains(body.trim()) {
            return Ok(false);
        }
        let close_marker = "<!-- /agent:exchange -->";
        let Some(close) = doc.find(close_marker) else {
            anyhow::bail!("document has no agent exchange component");
        };
        let mut replacement = String::new();
        let before = &doc[..close];
        replacement.push_str(before);
        if !before.ends_with('\n') {
            replacement.push('\n');
        }
        replacement.push_str(body.trim_end());
        replacement.push('\n');
        replacement.push_str(&doc[close..]);
        std::fs::write(file, replacement)?;
        Ok(true)
    }

    fn finish_commit(file: &Path) -> Result<()> {
        let content = std::fs::read_to_string(file)?;
        agent_doc_snapshot_io::checkpoint_document_baseline(
            file,
            &content,
            agent_doc_ops_log_io::log_op,
        )?;
        let _ = Self::commit_file(file)?;
        agent_doc_cycle_state_io::mark_committed(
            file,
            "sync_test_repair",
            Some(&content),
            Some(&content),
        )?;
        let _ = agent_doc_capture_io::mark_committed_with_current_content(file, &content);
        Ok(())
    }
}

#[cfg(test)]
impl SyncRuntimeEffects for TestSyncRuntimeEffects {
    fn resolve_current_document(&self, file: &Path, _source: &str) -> Result<String> {
        std::fs::read_to_string(file)
            .with_context(|| format!("test resolver read {}", file.display()))
    }

    fn write_current_document(&self, file: &Path, content: &str) -> Result<()> {
        std::fs::write(file, content)
            .with_context(|| format!("test writer write {}", file.display()))
    }

    fn commit(&self, file: &Path) -> Result<bool> {
        Self::commit_file(file)
    }

    fn detect_jb_cache_conflict_cancel_recoverable(&self, file: &Path) -> Result<bool> {
        Ok(matches!(
            agent_doc_snapshot_io::verify_snapshot_committed(file)?,
            agent_doc_snapshot_io::SnapshotCommitStatus::SnapshotDiffersFromHead { .. }
        ))
    }

    fn detect_uncommitted_closeout_drift(&self, _file: &Path) -> Result<Option<String>> {
        Ok(None)
    }

    fn repair(&self, file: &Path) -> Result<agent_doc_turn::repair::RepairOutcome> {
        let Some(capture) = agent_doc_capture_io::load_active(file)? else {
            return Ok(agent_doc_turn::repair::RepairOutcome::Noop);
        };
        let response = capture.response_body;
        let state = agent_doc_cycle_state_io::load(file)?;
        let phase = state
            .as_ref()
            .map(|state| state.phase)
            .unwrap_or(agent_doc_turn::CyclePhase::ResponseCaptured);
        let outcome = match phase {
            agent_doc_turn::CyclePhase::WriteApplied => {
                Self::finish_commit(file)?;
                agent_doc_turn::repair::RepairOutcome::AlreadyApplied
            }
            _ => {
                let applied = Self::apply_exchange_patch(file, &response)?;
                Self::finish_commit(file)?;
                if applied {
                    let _ = agent_doc_capture_io::mark_replayed(file);
                    agent_doc_turn::repair::RepairOutcome::ReplayedResponse
                } else {
                    agent_doc_turn::repair::RepairOutcome::AlreadyApplied
                }
            }
        };
        Ok(outcome)
    }

    fn repair_stale_preflight_started_cycle(
        &self,
        file: &Path,
    ) -> Result<agent_doc_turn::repair::RepairOutcome> {
        let content = std::fs::read_to_string(file).unwrap_or_default();
        agent_doc_cycle_state_io::mark_committed(
            file,
            "stale_preflight_lock_repaired",
            Some(&content),
            Some(&content),
        )?;
        Ok(agent_doc_turn::repair::RepairOutcome::StalePreflightLockRepaired)
    }

    fn save_pending(&self, file: &Path, response: &str) -> Result<()> {
        let current_content =
            self.resolve_current_document(file, "sync_test_save_pending_capture")?;
        agent_doc_capture_io::capture_response_with_current_content(
            file,
            response,
            &current_content,
        )?;
        Ok(())
    }

    fn session_check_inspect(&self, file: &Path) -> Result<SyncSessionCheckStatus> {
        if agent_doc_capture_io::load_active(file)?.is_some() {
            return Ok(SyncSessionCheckStatus::Interrupted(
                "pending response remains".to_string(),
            ));
        }
        if agent_doc_cycle_state_io::load(file)?
            .as_ref()
            .is_some_and(|state| state.phase.is_open())
        {
            return Ok(SyncSessionCheckStatus::Interrupted(
                "cycle remains open".to_string(),
            ));
        }
        Ok(SyncSessionCheckStatus::Ok("clean".to_string()))
    }

    fn resolve_cross_root_document_pane(
        &self,
        project_root: &Path,
        file: &Path,
    ) -> Result<Option<agent_doc_controller_io::project_controller::ControllerTmuxActorBinding>>
    {
        Ok(
            agent_doc_session_registry_io::lookup_file_entry_in(project_root, file)?.map(|entry| {
                agent_doc_controller_io::project_controller::ControllerTmuxActorBinding {
                    document_path: file.display().to_string(),
                    session_id: entry.session_id,
                    pane_id: entry.pane,
                    generation: 0,
                }
            }),
        )
    }

    fn ensure_cross_root_document_pane(
        &self,
        project_root: &Path,
        file: &Path,
    ) -> Result<Option<String>> {
        Ok(
            agent_doc_session_registry_io::lookup_file_entry_in(project_root, file)?
                .map(|entry| entry.pane),
        )
    }

    fn provision_pane(
        &self,
        tmux: &tmux_router::Tmux,
        _file: &Path,
        session_id: &str,
        _file_path: &str,
        _context_session: Option<&str>,
        _col_args: &[String],
        _route_after_start: bool,
    ) -> Result<String> {
        if let Some(pane) = agent_doc_session_registry_io::lookup(session_id)?
            && tmux.pane_alive(&pane)
        {
            return Ok(pane);
        }
        anyhow::bail!("test sync runtime cannot provision a new pane")
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use parking_lot::MutexGuard;

    thread_local! {
        static PROCESS_GLOBAL_LOCK_DEPTH: std::cell::Cell<usize> =
            const { std::cell::Cell::new(0) };
    }

    pub(crate) struct ProcessGlobalLockGuard {
        _guard: Option<MutexGuard<'static, ()>>,
    }

    impl Drop for ProcessGlobalLockGuard {
        fn drop(&mut self) {
            PROCESS_GLOBAL_LOCK_DEPTH.with(|depth| {
                let current = depth.get();
                debug_assert!(current > 0, "process-global test lock depth underflow");
                depth.set(current.saturating_sub(1));
            });
        }
    }

    pub(crate) fn env_lock() -> ProcessGlobalLockGuard {
        let already_held = PROCESS_GLOBAL_LOCK_DEPTH.with(|depth| {
            let current = depth.get();
            depth.set(current + 1);
            current > 0
        });
        if already_held {
            return ProcessGlobalLockGuard { _guard: None };
        }

        let guard = agent_doc_harness::prompt_source::TEST_ENV_LOCK.lock();
        ProcessGlobalLockGuard {
            _guard: Some(guard),
        }
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
