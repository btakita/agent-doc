//! # Module: tmux_scope_io (`#tmuxscopesteward`)
//!
//! ## Spec
//! - tmux built with systemd support moves every pane process into its own
//!   transient `tmux-spawn-<uuid>.scope` in the user manager. When the pane
//!   process exits before systemd attaches it, the unit fails with
//!   `result=resources` and stays LOADED until `reset-failed`; when a process
//!   daemonizes out of a pane, it keeps the scope running after its tmux server
//!   is gone. Both accumulate in the user manager (3,911 loaded scopes for ~50
//!   live panes were observed before an X11 session crash on 2026-10-16).
//! - [`sweep`] is the binary-owned steward: it resets failed `tmux-spawn-*`
//!   scopes, keeps scopes whose launching tmux server is alive, stops orphaned
//!   scopes only when every process left in them is a provable agent-doc test
//!   fixture leak, and reports everything else without touching it.
//! - [`run_scheduled_sweep`] rate-limits the sweep host-wide (one stamp file per
//!   user under `$XDG_RUNTIME_DIR`), so any number of project controllers share
//!   one cadence. It is a no-op off Linux, without a systemd user manager, or
//!   with `AGENT_DOC_TMUX_SCOPE_SWEEP=0`.
//!
//! ## Agentic Contracts
//! - Only units named `tmux-spawn-*.scope` are ever read or acted on.
//! - No process outside a provable agent-doc fixture leak is ever signalled.
//! - Every sweep returns a [`ScopeSweepReport`]; callers log
//!   [`ScopeSweepReport::log_lines`] to `ops.log`.

pub mod classify;

pub use classify::{
    ClassifyPolicy, LauncherState, ScopeObservation, ScopeProcess, ScopeUnit, ScopeVerdict,
    TMUX_SPAWN_SCOPE_GLOB, classify_scope, parse_systemctl_show,
};

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Set to `0`/`off`/`false` to disable the automatic (scheduled) sweep.
pub const SWEEP_ENABLE_ENV: &str = "AGENT_DOC_TMUX_SCOPE_SWEEP";
/// Override (seconds) for the host-wide automatic sweep cadence.
pub const SWEEP_INTERVAL_SECS_ENV: &str = "AGENT_DOC_TMUX_SCOPE_SWEEP_INTERVAL_SECS";
/// Default host-wide cadence of the automatic sweep.
pub const DEFAULT_SWEEP_INTERVAL: Duration = Duration::from_secs(600);
/// A fixture process whose root still exists must be this old to be stopped.
pub const DEFAULT_MIN_LEAK_AGE: Duration = Duration::from_secs(600);
/// Upper bound on scopes stopped by one sweep (bounded blast radius).
pub const MAX_STOPS_PER_SWEEP: usize = 64;
/// Upper bound on per-scope `ops.log` lines one sweep writes.
pub const MAX_DETAIL_LOG_LINES: usize = 20;
const SYSTEMCTL_TIMEOUT: Duration = Duration::from_secs(20);
const UNITS_PER_SYSTEMCTL_CALL: usize = 256;

/// One scope the sweep stopped or reported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScopeAction {
    pub unit: String,
    pub reason: String,
    pub pane_pid: Option<u32>,
    pub launcher_pid: Option<u32>,
    pub pids: Vec<u32>,
}

/// Deterministic result of one sweep.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ScopeSweepReport {
    pub dry_run: bool,
    /// `None` when the sweep ran; otherwise why it could not.
    pub unavailable: Option<String>,
    pub scopes_before: usize,
    pub failed_before: usize,
    pub running_before: usize,
    pub launcher_alive: usize,
    /// Failed units unloaded (or, on a dry run, that would be).
    pub reset: Vec<String>,
    pub stopped: Vec<ScopeAction>,
    pub reported: Vec<ScopeAction>,
    /// Loaded `tmux-spawn-*` scopes after the sweep (`None` on a dry run).
    pub scopes_after: Option<usize>,
    pub errors: Vec<String>,
}

impl ScopeSweepReport {
    fn unavailable(dry_run: bool, reason: impl Into<String>) -> Self {
        Self {
            dry_run,
            unavailable: Some(reason.into()),
            ..Self::default()
        }
    }

    /// The summary line plus bounded per-scope detail lines for `ops.log`.
    pub fn log_lines(&self, trigger: &str) -> Vec<String> {
        let mut lines = vec![self.summary_line(trigger)];
        let details = self
            .stopped
            .iter()
            .map(|action| ("stopped", action))
            .chain(self.reported.iter().map(|action| ("reported", action)));
        for (kind, action) in details.take(MAX_DETAIL_LOG_LINES) {
            lines.push(format!(
                "tmux_scope_sweep_{kind} unit={} reason={} pane_pid={} launcher_pid={} pids={}",
                action.unit,
                action.reason.replace(' ', "_"),
                optional(action.pane_pid),
                optional(action.launcher_pid),
                join_pids(&action.pids),
            ));
        }
        lines
    }

    pub fn summary_line(&self, trigger: &str) -> String {
        if let Some(reason) = &self.unavailable {
            return format!(
                "tmux_scope_sweep trigger={trigger} dry_run={} status=unavailable reason={reason}",
                self.dry_run
            );
        }
        format!(
            "tmux_scope_sweep trigger={trigger} dry_run={} status=ran scopes_before={} failed_before={} running_before={} launcher_alive={} reset={} stopped={} reported={} scopes_after={} errors={}",
            self.dry_run,
            self.scopes_before,
            self.failed_before,
            self.running_before,
            self.launcher_alive,
            self.reset.len(),
            self.stopped.len(),
            self.reported.len(),
            self.scopes_after
                .map(|count| count.to_string())
                .unwrap_or_else(|| "n/a".to_string()),
            if self.errors.is_empty() {
                "none".to_string()
            } else {
                self.errors.join(" | ").replace(' ', "_")
            },
        )
    }
}

fn optional(value: Option<u32>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "none".to_string())
}

fn join_pids(pids: &[u32]) -> String {
    if pids.is_empty() {
        return "none".to_string();
    }
    pids.iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// The IO seam between the sweep policy and the host.
pub trait ScopeHost {
    /// `systemctl --user show` output for every loaded `tmux-spawn-*` scope.
    fn show_scopes(&self) -> Result<String, String>;
    /// `/proc/<pid>/comm`, or `None` when no such process exists.
    fn process_comm(&self, pid: u32) -> Option<String>;
    /// Processes in a scope's cgroup, or `None` when it cannot be read.
    fn cgroup_processes(&self, control_group: &str) -> Option<Vec<ScopeProcess>>;
    fn path_exists(&self, path: &Path) -> bool;
    fn reset_failed(&self, units: &[String]) -> Result<(), String>;
    fn stop(&self, units: &[String]) -> Result<(), String>;
}

/// Run one sweep against `host`.
pub fn sweep_with_host(
    host: &dyn ScopeHost,
    policy: &ClassifyPolicy,
    dry_run: bool,
) -> ScopeSweepReport {
    let text = match host.show_scopes() {
        Ok(text) => text,
        Err(error) => return ScopeSweepReport::unavailable(dry_run, error),
    };
    let units = parse_systemctl_show(&text);
    let mut report = ScopeSweepReport {
        dry_run,
        scopes_before: units.len(),
        failed_before: units.iter().filter(|unit| unit.is_failed()).count(),
        running_before: units.iter().filter(|unit| unit.is_running()).count(),
        ..ScopeSweepReport::default()
    };
    let mut to_stop = Vec::new();
    for unit in units {
        let pids = unit.pids();
        let launcher = match pids {
            Some((_, launcher)) if unit.is_running() => {
                classify::launcher_state_from_comm(host.process_comm(launcher).as_deref())
            }
            Some(_) => LauncherState::Gone,
            None => LauncherState::Unknown,
        };
        // Every running scope's cgroup is read: a live launcher alone does
        // not prove the pane (and not a daemon it left behind) holds it.
        let processes = unit
            .is_running()
            .then(|| host.cgroup_processes(&unit.control_group))
            .flatten();
        let observation = ScopeObservation {
            unit,
            launcher,
            processes,
        };
        let verdict = classify_scope(&observation, policy, &|path| host.path_exists(path));
        let action = |reason: String, pids_in_scope: Vec<u32>| ScopeAction {
            unit: observation.unit.id.clone(),
            reason,
            pane_pid: pids.map(|(pane, _)| pane),
            launcher_pid: pids.map(|(_, launcher)| launcher),
            pids: pids_in_scope,
        };
        match verdict {
            ScopeVerdict::ResetFailed => report.reset.push(observation.unit.id.clone()),
            ScopeVerdict::Keep { reason } => {
                if reason == "launcher_alive" {
                    report.launcher_alive += 1;
                }
            }
            ScopeVerdict::StopLeak { reason, pids } => {
                if to_stop.len() < MAX_STOPS_PER_SWEEP {
                    to_stop.push(action(reason, pids));
                } else {
                    report
                        .reported
                        .push(action(format!("{reason}_deferred_stop_cap"), pids));
                }
            }
            ScopeVerdict::Report { reason, pids } => report.reported.push(action(reason, pids)),
        }
    }
    if dry_run {
        report.stopped = to_stop;
        return report;
    }
    for chunk in report.reset.chunks(UNITS_PER_SYSTEMCTL_CALL) {
        if let Err(error) = host.reset_failed(chunk) {
            report.errors.push(format!("reset_failed: {error}"));
        }
    }
    if !to_stop.is_empty() {
        let units: Vec<String> = to_stop.iter().map(|action| action.unit.clone()).collect();
        match host.stop(&units) {
            Ok(()) => report.stopped = to_stop,
            Err(error) => {
                report.errors.push(format!("stop: {error}"));
                report
                    .reported
                    .extend(to_stop.into_iter().map(|mut action| {
                        action.reason = format!("{}_stop_failed", action.reason);
                        action
                    }));
            }
        }
    }
    report.scopes_after = host
        .show_scopes()
        .ok()
        .map(|text| parse_systemctl_show(&text).len());
    report
}

/// The policy used by the real sweep.
pub fn default_policy() -> ClassifyPolicy {
    ClassifyPolicy::with_temp_bases(&std::env::temp_dir(), DEFAULT_MIN_LEAK_AGE)
}

/// Run one sweep against this host's systemd user manager.
pub fn sweep(dry_run: bool) -> ScopeSweepReport {
    #[cfg(target_os = "linux")]
    {
        if !Path::new("/run/systemd/system").is_dir() {
            return ScopeSweepReport::unavailable(dry_run, "not_booted_with_systemd");
        }
        sweep_with_host(&system::SystemScopeHost::new(), &default_policy(), dry_run)
    }
    #[cfg(not(target_os = "linux"))]
    {
        ScopeSweepReport::unavailable(dry_run, "not_linux")
    }
}

/// Is the automatic sweep enabled by the environment?
pub fn scheduled_sweep_enabled(value: Option<&str>) -> bool {
    !matches!(
        value
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref(),
        Some("0" | "off" | "false" | "no")
    )
}

/// The automatic sweep cadence from the environment override.
pub fn scheduled_sweep_interval(value: Option<&str>) -> Duration {
    value
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_SWEEP_INTERVAL)
}

/// Whether a sweep is due given the last recorded sweep (epoch secs).
///
/// A stamp from the future (clock stepped backwards) counts as due, so a bad
/// clock can delay the steward by at most one call, never forever.
pub fn sweep_due(last_epoch_secs: Option<u64>, now_epoch_secs: u64, interval: Duration) -> bool {
    match last_epoch_secs {
        None => true,
        Some(last) if last > now_epoch_secs => true,
        Some(last) => now_epoch_secs - last >= interval.as_secs(),
    }
}

/// Directory holding the host-wide sweep stamp for this user.
pub fn stamp_dir() -> PathBuf {
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from)
        && runtime.is_absolute()
        && runtime.is_dir()
    {
        return runtime.join("agent-doc");
    }
    std::env::temp_dir().join(format!("agent-doc-{}", current_uid()))
}

fn current_uid() -> u32 {
    #[cfg(unix)]
    {
        // SAFETY: getuid has no preconditions and cannot fail.
        unsafe { libc::getuid() }
    }
    #[cfg(not(unix))]
    {
        0
    }
}

fn now_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Claim the host-wide sweep slot under `dir` if one is due.
///
/// Holds an exclusive non-blocking `flock` while reading and advancing the
/// stamp, so concurrent controllers cannot both win the same slot. The stamp is
/// advanced BEFORE the sweep runs: a sweep that crashes or hangs costs one
/// interval, never a hot retry loop.
pub fn claim_sweep_slot(
    dir: &Path,
    now_epoch_secs: u64,
    interval: Duration,
) -> Result<bool, String> {
    use fs2::FileExt;
    use std::io::{Read, Seek, Write};
    std::fs::create_dir_all(dir)
        .map_err(|error| format!("sweep_stamp_mkdir_failed:{}:{error}", dir.display()))?;
    let stamp = dir.join("tmux-scope-sweep.stamp");
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&stamp)
        .map_err(|error| format!("sweep_stamp_open_failed:{}:{error}", stamp.display()))?;
    if let Err(error) = file.try_lock_exclusive() {
        return if error.kind() == std::io::ErrorKind::WouldBlock {
            Ok(false)
        } else {
            Err(format!(
                "sweep_stamp_lock_failed:{}:{error}",
                stamp.display()
            ))
        };
    };
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|error| format!("sweep_stamp_read_failed:{}:{error}", stamp.display()))?;
    let trimmed = contents.trim();
    let last = if trimmed.is_empty() {
        None
    } else {
        Some(
            trimmed
                .parse::<u64>()
                .map_err(|error| format!("sweep_stamp_parse_failed:{}:{error}", stamp.display()))?,
        )
    };
    if !sweep_due(last, now_epoch_secs, interval) {
        return Ok(false);
    }
    file.set_len(0)
        .map_err(|error| format!("sweep_stamp_truncate_failed:{}:{error}", stamp.display()))?;
    file.rewind()
        .map_err(|error| format!("sweep_stamp_rewind_failed:{}:{error}", stamp.display()))?;
    writeln!(file, "{now_epoch_secs}")
        .map_err(|error| format!("sweep_stamp_write_failed:{}:{error}", stamp.display()))?;
    // Dropping `file` releases the advisory lock even if a later return path is
    // added. There is no explicit unlock whose failure could be hidden.
    Ok(true)
}

/// Rate-limited automatic sweep for long-lived agent-doc processes.
///
/// Returns `None` when disabled or not yet due (nothing is logged); otherwise
/// runs the sweep and feeds every log line to `log`.
pub fn run_scheduled_sweep(log: &mut dyn FnMut(&str)) -> Option<ScopeSweepReport> {
    if !scheduled_sweep_enabled(std::env::var(SWEEP_ENABLE_ENV).ok().as_deref()) {
        return None;
    }
    let interval = scheduled_sweep_interval(std::env::var(SWEEP_INTERVAL_SECS_ENV).ok().as_deref());
    let report = match claim_sweep_slot(&stamp_dir(), now_epoch_secs(), interval) {
        Ok(false) => return None,
        Ok(true) => sweep(false),
        Err(error) => ScopeSweepReport::unavailable(false, error),
    };
    for line in report.log_lines("scheduled") {
        log(&line);
    }
    Some(report)
}

#[cfg(target_os = "linux")]
mod system {
    use super::*;
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::Instant;

    pub struct SystemScopeHost {
        runtime_dir: Option<PathBuf>,
        uptime_secs: Option<f64>,
        clock_ticks: u64,
    }

    impl SystemScopeHost {
        pub fn new() -> Self {
            // A detached controller may lack XDG_RUNTIME_DIR; `systemctl --user`
            // finds the manager's private socket through it.
            let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
                .map(PathBuf::from)
                .filter(|dir| dir.is_dir())
                .or_else(|| {
                    let dir = PathBuf::from(format!("/run/user/{}", current_uid()));
                    dir.is_dir().then_some(dir)
                });
            let uptime_secs = std::fs::read_to_string("/proc/uptime")
                .ok()
                .and_then(|text| text.split_whitespace().next()?.parse::<f64>().ok());
            // SAFETY: sysconf has no preconditions.
            let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
            Self {
                runtime_dir,
                uptime_secs,
                clock_ticks: if ticks > 0 { ticks as u64 } else { 100 },
            }
        }

        fn systemctl(&self, args: &[&str], units: &[String]) -> Result<String, String> {
            let mut command = Command::new("systemctl");
            command.arg("--user").args(args).arg("--").args(units);
            if let Some(dir) = &self.runtime_dir {
                command.env("XDG_RUNTIME_DIR", dir);
            }
            run_bounded(command, SYSTEMCTL_TIMEOUT)
        }

        fn process(&self, pid: u32) -> Option<ScopeProcess> {
            let proc_dir = PathBuf::from(format!("/proc/{pid}"));
            let argv: Vec<String> = std::fs::read(proc_dir.join("cmdline"))
                .ok()?
                .split(|byte| *byte == 0)
                .filter(|arg| !arg.is_empty())
                .map(|arg| String::from_utf8_lossy(arg).into_owned())
                .collect();
            let exe_name = std::fs::read_link(proc_dir.join("exe"))
                .ok()
                .and_then(|exe| {
                    let name = exe.file_name()?.to_str()?.to_string();
                    Some(name.trim_end_matches(" (deleted)").to_string())
                });
            let cwd = std::fs::read_link(proc_dir.join("cwd")).ok();
            let age_secs = std::fs::read_to_string(proc_dir.join("stat"))
                .ok()
                .and_then(|stat| {
                    // Field 22 (starttime) counted after the `)` closing comm.
                    let start_ticks: u64 = stat
                        .rsplit_once(')')?
                        .1
                        .split_whitespace()
                        .nth(19)?
                        .parse()
                        .ok()?;
                    let uptime = self.uptime_secs?;
                    let started = start_ticks as f64 / self.clock_ticks as f64;
                    Some((uptime - started).max(0.0) as u64)
                });
            Some(ScopeProcess {
                pid,
                argv,
                exe_name,
                cwd,
                age_secs,
            })
        }
    }

    impl ScopeHost for SystemScopeHost {
        fn show_scopes(&self) -> Result<String, String> {
            self.systemctl(
                &[
                    "show",
                    "--property=Id,Description,LoadState,ActiveState,SubState,Result,ControlGroup",
                ],
                &[TMUX_SPAWN_SCOPE_GLOB.to_string()],
            )
        }

        fn process_comm(&self, pid: u32) -> Option<String> {
            std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()
        }

        fn cgroup_processes(&self, control_group: &str) -> Option<Vec<ScopeProcess>> {
            if !control_group.starts_with('/') || control_group.contains("..") {
                return None;
            }
            let procs =
                std::fs::read_to_string(format!("/sys/fs/cgroup{control_group}/cgroup.procs"))
                    .ok()?;
            let mut processes = Vec::new();
            for pid in procs
                .lines()
                .filter_map(|line| line.trim().parse::<u32>().ok())
            {
                // A process that exits between the two reads is simply gone.
                if let Some(process) = self.process(pid) {
                    processes.push(process);
                } else if Path::new(&format!("/proc/{pid}")).exists() {
                    return None;
                }
            }
            Some(processes)
        }

        fn path_exists(&self, path: &Path) -> bool {
            path.exists()
        }

        fn reset_failed(&self, units: &[String]) -> Result<(), String> {
            self.systemctl(&["reset-failed"], units).map(|_| ())
        }

        fn stop(&self, units: &[String]) -> Result<(), String> {
            self.systemctl(&["stop", "--no-block"], units).map(|_| ())
        }
    }

    /// Run a command with a deadline; the child is killed when it overruns.
    fn run_bounded(mut command: Command, timeout: Duration) -> Result<String, String> {
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("spawn_systemctl_failed:{error}"))?;
        let terminate = |child: &mut std::process::Child, reason: &str| -> String {
            let kill = child
                .kill()
                .map(|()| "killed".to_string())
                .unwrap_or_else(|error| format!("kill_failed:{error}"));
            let wait = child
                .wait()
                .map(|status| format!("waited:{status}"))
                .unwrap_or_else(|error| format!("wait_failed:{error}"));
            format!("{reason}:{kill}:{wait}")
        };
        let mut stdout = match child.stdout.take() {
            Some(stdout) => stdout,
            None => return Err(terminate(&mut child, "systemctl_stdout_pipe_missing")),
        };
        let mut stderr = match child.stderr.take() {
            Some(stderr) => stderr,
            None => return Err(terminate(&mut child, "systemctl_stderr_pipe_missing")),
        };
        let stdout_reader = match std::thread::Builder::new()
            .name("tmux-scope-systemctl-stdout".to_string())
            .spawn(move || {
                let mut text = String::new();
                stdout
                    .read_to_string(&mut text)
                    .map_err(|error| format!("systemctl_stdout_read_failed:{error}"))?;
                Ok::<String, String>(text)
            }) {
            Ok(reader) => reader,
            Err(error) => {
                let cleanup = terminate(&mut child, "systemctl_stdout_reader_spawn_failed");
                return Err(format!("{cleanup}:{error}"));
            }
        };
        let stderr_reader = match std::thread::Builder::new()
            .name("tmux-scope-systemctl-stderr".to_string())
            .spawn(move || {
                let mut text = String::new();
                stderr
                    .read_to_string(&mut text)
                    .map_err(|error| format!("systemctl_stderr_read_failed:{error}"))?;
                Ok::<String, String>(text)
            }) {
            Ok(reader) => reader,
            Err(error) => {
                let cleanup = terminate(&mut child, "systemctl_stderr_reader_spawn_failed");
                let stdout_result = stdout_reader
                    .join()
                    .map_err(|_| "systemctl_stdout_reader_panicked".to_string())?;
                return Err(format!(
                    "{cleanup}:{error}:stdout={}",
                    reader_outcome(&stdout_result)
                ));
            }
        };
        let join_reader = |reader: std::thread::JoinHandle<Result<String, String>>, name: &str| {
            reader
                .join()
                .map_err(|_| format!("systemctl_{name}_reader_panicked"))?
        };
        let deadline = Instant::now() + timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() >= deadline => {
                    let cleanup = terminate(&mut child, "systemctl_timeout");
                    let stdout_result = join_reader(stdout_reader, "stdout");
                    let stderr_result = join_reader(stderr_reader, "stderr");
                    return Err(format!(
                        "{cleanup}:stdout={}:stderr={}",
                        reader_outcome(&stdout_result),
                        reader_outcome(&stderr_result)
                    ));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                Err(error) => {
                    let cleanup = terminate(&mut child, "systemctl_wait_failed");
                    let stdout_result = join_reader(stdout_reader, "stdout");
                    let stderr_result = join_reader(stderr_reader, "stderr");
                    return Err(format!(
                        "{cleanup}:{error}:stdout={}:stderr={}",
                        reader_outcome(&stdout_result),
                        reader_outcome(&stderr_result)
                    ));
                }
            }
        };
        let stdout = join_reader(stdout_reader, "stdout")?;
        let stderr = join_reader(stderr_reader, "stderr")?;
        if status.success() {
            Ok(stdout)
        } else {
            Err(format!(
                "systemctl_exit_{}:{}",
                status.code().unwrap_or(-1),
                stderr.trim().replace('\n', " ")
            ))
        }
    }

    fn reader_outcome(result: &Result<String, String>) -> String {
        match result {
            Ok(text) if text.is_empty() => "empty".to_string(),
            Ok(text) => format!("bytes_{}", text.len()),
            Err(error) => error.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::{BTreeMap, BTreeSet};

    /// In-memory systemd user manager + process table (a SimWorld for scopes).
    #[derive(Default)]
    struct SimHost {
        units: RefCell<BTreeMap<String, ScopeUnit>>,
        tmux_servers: BTreeSet<u32>,
        other_processes: BTreeMap<u32, String>,
        cgroups: BTreeMap<String, Vec<ScopeProcess>>,
        existing_paths: BTreeSet<PathBuf>,
        stopped: RefCell<Vec<String>>,
        fail_stop: bool,
        show_error: Option<String>,
    }

    impl SimHost {
        fn add_unit(&mut self, id: &str, pane: u32, launcher: u32, state: &str) {
            let unit = ScopeUnit {
                id: id.to_string(),
                description: format!("tmux child pane {pane} launched by process {launcher}"),
                load_state: "loaded".to_string(),
                active_state: state.to_string(),
                sub_state: if state == "failed" {
                    "failed"
                } else {
                    "running"
                }
                .to_string(),
                result: if state == "failed" {
                    "resources"
                } else {
                    "success"
                }
                .to_string(),
                control_group: if state == "failed" {
                    String::new()
                } else {
                    format!("/user.slice/{id}")
                },
            };
            self.units.borrow_mut().insert(id.to_string(), unit);
        }

        fn render_show(&self) -> String {
            self.units
                .borrow()
                .values()
                .map(|unit| {
                    format!(
                        "Id={}\nDescription={}\nLoadState={}\nActiveState={}\nSubState={}\nResult={}\nControlGroup={}\n",
                        unit.id,
                        unit.description,
                        unit.load_state,
                        unit.active_state,
                        unit.sub_state,
                        unit.result,
                        unit.control_group
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
    }

    impl ScopeHost for SimHost {
        fn show_scopes(&self) -> Result<String, String> {
            match &self.show_error {
                Some(error) => Err(error.clone()),
                None => Ok(self.render_show()),
            }
        }
        fn process_comm(&self, pid: u32) -> Option<String> {
            if self.tmux_servers.contains(&pid) {
                return Some("tmux: server\n".to_string());
            }
            self.other_processes.get(&pid).cloned()
        }
        fn cgroup_processes(&self, control_group: &str) -> Option<Vec<ScopeProcess>> {
            self.cgroups.get(control_group).cloned()
        }
        fn path_exists(&self, path: &Path) -> bool {
            self.existing_paths.contains(path)
        }
        fn reset_failed(&self, units: &[String]) -> Result<(), String> {
            let mut loaded = self.units.borrow_mut();
            for unit in units {
                if loaded.get(unit).is_some_and(ScopeUnit::is_failed) {
                    loaded.remove(unit);
                }
            }
            Ok(())
        }
        fn stop(&self, units: &[String]) -> Result<(), String> {
            if self.fail_stop {
                return Err("denied".to_string());
            }
            let mut loaded = self.units.borrow_mut();
            for unit in units {
                loaded.remove(unit);
                self.stopped.borrow_mut().push(unit.clone());
            }
            Ok(())
        }
    }

    fn fixture_controller(pid: u32, root: &str) -> ScopeProcess {
        ScopeProcess {
            pid,
            argv: vec![
                "agent-doc".to_string(),
                "controller".to_string(),
                "serve".to_string(),
                "--project-root".to_string(),
                root.to_string(),
            ],
            exe_name: Some("agent-doc".to_string()),
            cwd: Some(PathBuf::from("/")),
            age_secs: Some(5),
        }
    }

    fn policy() -> ClassifyPolicy {
        ClassifyPolicy::with_temp_bases(Path::new("/tmp"), DEFAULT_MIN_LEAK_AGE)
    }

    /// The 2026-10-16 shape: one live operator server, a burst of failed test
    /// scopes, an orphaned scope pinned by a leaked fixture controller, and an
    /// orphaned scope still holding an operator shell.
    fn leaky_world() -> SimHost {
        let mut host = SimHost::default();
        host.tmux_servers.insert(7004);
        host.add_unit("tmux-spawn-live.scope", 7085, 7004, "active");
        for index in 0..300 {
            host.add_unit(
                &format!("tmux-spawn-failed-{index}.scope"),
                2_000 + index,
                1_999,
                "failed",
            );
        }
        host.add_unit("tmux-spawn-leak.scope", 3_001, 3_000, "active");
        host.cgroups.insert(
            "/user.slice/tmux-spawn-leak.scope".to_string(),
            vec![fixture_controller(3_050, "/tmp/.tmpGone/project")],
        );
        host.add_unit("tmux-spawn-operator.scope", 4_001, 4_000, "active");
        host.other_processes.insert(4_000, "bash\n".to_string());
        host.cgroups.insert(
            "/user.slice/tmux-spawn-operator.scope".to_string(),
            vec![ScopeProcess {
                pid: 4_002,
                argv: vec!["nvim".to_string(), "/home/u/notes.md".to_string()],
                exe_name: Some("nvim".to_string()),
                cwd: Some(PathBuf::from("/home/u")),
                age_secs: Some(99_999),
            }],
        );
        host
    }

    #[test]
    fn sweep_resets_failed_stops_fixture_leak_and_reports_operator_scope() {
        let host = leaky_world();
        let report = sweep_with_host(&host, &policy(), false);
        assert_eq!(report.unavailable, None);
        assert_eq!(report.scopes_before, 303);
        assert_eq!(report.failed_before, 300);
        assert_eq!(report.running_before, 3);
        assert_eq!(report.launcher_alive, 1);
        assert_eq!(report.reset.len(), 300);
        assert_eq!(
            report
                .stopped
                .iter()
                .map(|a| a.unit.as_str())
                .collect::<Vec<_>>(),
            vec!["tmux-spawn-leak.scope"]
        );
        assert_eq!(report.stopped[0].pids, vec![3_050]);
        assert_eq!(report.reported.len(), 1);
        assert_eq!(report.reported[0].unit, "tmux-spawn-operator.scope");
        assert_eq!(report.scopes_after, Some(2));
        assert!(report.errors.is_empty());
        assert_eq!(
            *host.stopped.borrow(),
            vec!["tmux-spawn-leak.scope".to_string()]
        );
        let units = host.units.borrow();
        assert!(units.contains_key("tmux-spawn-live.scope"));
        assert!(units.contains_key("tmux-spawn-operator.scope"));
    }

    #[test]
    fn dry_run_changes_nothing_but_reports_the_plan() {
        let host = leaky_world();
        let report = sweep_with_host(&host, &policy(), true);
        assert!(report.dry_run);
        assert_eq!(report.reset.len(), 300);
        assert_eq!(report.stopped.len(), 1);
        assert_eq!(report.scopes_after, None);
        assert_eq!(host.units.borrow().len(), 303);
        assert!(host.stopped.borrow().is_empty());
    }

    #[test]
    fn failed_stop_is_reported_not_claimed() {
        let mut host = leaky_world();
        host.fail_stop = true;
        let report = sweep_with_host(&host, &policy(), false);
        assert!(report.stopped.is_empty());
        assert!(
            report
                .reported
                .iter()
                .any(|a| a.unit == "tmux-spawn-leak.scope" && a.reason.ends_with("_stop_failed"))
        );
        assert_eq!(report.errors, vec!["stop: denied".to_string()]);
    }

    #[test]
    fn stop_count_is_capped_per_sweep() {
        let mut host = SimHost::default();
        for index in 0..(MAX_STOPS_PER_SWEEP as u32 + 3) {
            let id = format!("tmux-spawn-leak-{index:03}.scope");
            host.add_unit(&id, 10_000 + index, 9_000 + index, "active");
            host.cgroups.insert(
                format!("/user.slice/{id}"),
                vec![fixture_controller(20_000 + index, "/tmp/.tmpX")],
            );
        }
        let report = sweep_with_host(&host, &policy(), false);
        assert_eq!(report.stopped.len(), MAX_STOPS_PER_SWEEP);
        assert_eq!(report.reported.len(), 3);
        assert!(
            report
                .reported
                .iter()
                .all(|a| a.reason.ends_with("_deferred_stop_cap"))
        );
    }

    #[test]
    fn unavailable_manager_is_a_reported_no_op() {
        let host = SimHost {
            show_error: Some("systemctl_exit_1:Failed to connect to bus".to_string()),
            ..SimHost::default()
        };
        let report = sweep_with_host(&host, &policy(), false);
        assert!(report.unavailable.is_some());
        assert!(report.log_lines("admin")[0].contains("status=unavailable"));
    }

    #[test]
    fn log_lines_carry_counts_and_bounded_detail() {
        let host = leaky_world();
        let report = sweep_with_host(&host, &policy(), false);
        let lines = report.log_lines("scheduled");
        assert_eq!(
            lines[0],
            "tmux_scope_sweep trigger=scheduled dry_run=false status=ran scopes_before=303 failed_before=300 running_before=3 launcher_alive=1 reset=300 stopped=1 reported=1 scopes_after=2 errors=none"
        );
        assert!(lines[1].starts_with("tmux_scope_sweep_stopped unit=tmux-spawn-leak.scope "));
        assert!(lines[2].starts_with("tmux_scope_sweep_reported unit=tmux-spawn-operator.scope "));
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn sweep_due_handles_first_run_interval_and_clock_skew() {
        let interval = Duration::from_secs(600);
        assert!(sweep_due(None, 1_000, interval));
        assert!(!sweep_due(Some(1_000), 1_599, interval));
        assert!(sweep_due(Some(1_000), 1_600, interval));
        assert!(sweep_due(Some(5_000), 1_000, interval));
    }

    #[test]
    fn env_parsing_defaults_and_disables() {
        assert!(scheduled_sweep_enabled(None));
        assert!(scheduled_sweep_enabled(Some("1")));
        assert!(!scheduled_sweep_enabled(Some("0")));
        assert!(!scheduled_sweep_enabled(Some(" OFF ")));
        assert_eq!(scheduled_sweep_interval(None), DEFAULT_SWEEP_INTERVAL);
        assert_eq!(scheduled_sweep_interval(Some("0")), DEFAULT_SWEEP_INTERVAL);
        assert_eq!(
            scheduled_sweep_interval(Some("30")),
            Duration::from_secs(30)
        );
    }

    #[test]
    fn sweep_slot_is_claimed_once_per_interval() {
        let dir = tempfile::tempdir().unwrap();
        let interval = Duration::from_secs(600);
        assert_eq!(claim_sweep_slot(dir.path(), 10_000, interval), Ok(true));
        assert_eq!(claim_sweep_slot(dir.path(), 10_100, interval), Ok(false));
        assert_eq!(claim_sweep_slot(dir.path(), 10_600, interval), Ok(true));
        let stamp = std::fs::read_to_string(dir.path().join("tmux-scope-sweep.stamp")).unwrap();
        assert_eq!(stamp.trim(), "10600");
    }

    #[test]
    fn sweep_slot_is_refused_while_another_holder_has_the_lock() {
        use fs2::FileExt;
        let dir = tempfile::tempdir().unwrap();
        let holder = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.path().join("tmux-scope-sweep.stamp"))
            .unwrap();
        holder.lock_exclusive().unwrap();
        assert_eq!(
            claim_sweep_slot(dir.path(), 10_000, Duration::from_secs(600)),
            Ok(false)
        );
        FileExt::unlock(&holder).unwrap();
        assert_eq!(
            claim_sweep_slot(dir.path(), 10_000, Duration::from_secs(600)),
            Ok(true)
        );
    }

    #[test]
    fn corrupt_sweep_stamp_is_reported_instead_of_silently_disabling_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("tmux-scope-sweep.stamp"), "not-an-epoch\n").unwrap();
        let error = claim_sweep_slot(dir.path(), 10_000, Duration::from_secs(600)).unwrap_err();
        assert!(error.starts_with("sweep_stamp_parse_failed:"));
    }
}
