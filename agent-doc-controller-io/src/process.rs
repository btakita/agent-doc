use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessSignal {
    Term,
    Kill,
}

impl ProcessSignal {
    fn as_arg(self) -> &'static str {
        match self {
            Self::Term => "-TERM",
            Self::Kill => "-KILL",
        }
    }
}

pub fn process_pids() -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut pids = BTreeSet::new();
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let Ok(pid) = name.parse::<u32>() else {
            continue;
        };
        pids.insert(pid);
    }
    pids.into_iter().collect()
}

pub fn process_is_alive(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

pub fn process_start_age_secs(pid: u32) -> Option<u64> {
    let modified = std::fs::metadata(format!("/proc/{pid}"))
        .ok()?
        .modified()
        .ok()?;
    SystemTime::now()
        .duration_since(modified)
        .ok()
        .map(|elapsed| elapsed.as_secs())
}

/// How far a pid's observed start time may sit after a recorded claim before the
/// pid is treated as reused. Boot time is derived as `now - /proc/uptime`, which
/// drifts by about a second between derivations (`#ctrliotestflake`), so a margin
/// inside that jitter would misread the original process as a newcomer.
const PID_REUSE_START_SLACK_SECS: u64 = 3;

/// Liveness of a process recorded as owning something since `recorded_since_secs`.
///
/// The recorded process was necessarily running when it recorded the claim, so
/// a live pid whose process *started after* the claim is a different process
/// that inherited a recycled pid: the recorded owner is gone. A zombie has also
/// exited; only its parent has not reaped it. When the start time cannot be
/// read (non-Linux, or a permission/race failure) this falls back to plain pid
/// existence, which keeps the conservative answer — never report an owner dead
/// that cannot be proven dead.
pub fn recorded_process_is_alive(pid: u32, recorded_since_secs: u64) -> bool {
    if pid == 0 || !process_is_alive(pid) {
        return false;
    }
    let Some(stat) = read_proc_stat(pid) else {
        return process_is_alive(pid);
    };
    let boot_secs = system_boot_timestamp_secs(
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or_default(),
    );
    recorded_process_alive_from_stat(&stat, clock_ticks_per_sec(), boot_secs, recorded_since_secs)
}

/// The pure decision behind [`recorded_process_is_alive`], over one
/// `/proc/<pid>/stat` line.
fn recorded_process_alive_from_stat(
    stat: &str,
    ticks_per_sec: Option<u64>,
    boot_secs: Option<u64>,
    recorded_since_secs: u64,
) -> bool {
    let Some((state, start_ticks)) = parse_proc_stat_state_and_start(stat) else {
        return true;
    };
    if matches!(state, 'Z' | 'X' | 'x') {
        return false;
    }
    let (Some(ticks_per_sec), Some(boot_secs)) = (ticks_per_sec, boot_secs) else {
        return true;
    };
    if ticks_per_sec == 0 {
        return true;
    }
    let started_secs = boot_secs.saturating_add(start_ticks / ticks_per_sec);
    started_secs <= recorded_since_secs.saturating_add(PID_REUSE_START_SLACK_SECS)
}

/// `(state, starttime)` from a `/proc/<pid>/stat` line. `comm` (field 2) may
/// contain spaces and parentheses, so fields are counted from the last `)`.
fn parse_proc_stat_state_and_start(stat: &str) -> Option<(char, u64)> {
    let rest = &stat[stat.rfind(')')? + 1..];
    let mut fields = rest.split_whitespace();
    let state = fields.next()?.chars().next()?;
    // After `state` (field 3), `starttime` is field 22: skip fields 4..=21.
    let start_ticks = fields.nth(18)?.parse::<u64>().ok()?;
    Some((state, start_ticks))
}

fn read_proc_stat(pid: u32) -> Option<String> {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()
}

#[cfg(unix)]
fn clock_ticks_per_sec() -> Option<u64> {
    // SAFETY: `sysconf` reads a process-constant configuration value.
    let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    u64::try_from(ticks).ok().filter(|ticks| *ticks > 0)
}

#[cfg(not(unix))]
fn clock_ticks_per_sec() -> Option<u64> {
    None
}

pub fn system_boot_timestamp_secs(now_secs: u64) -> Option<u64> {
    let uptime = std::fs::read_to_string("/proc/uptime").ok()?;
    system_boot_timestamp_secs_from_uptime(now_secs, &uptime)
}

pub fn is_same_project_controller_pid(project_root: &Path, pid: u32) -> bool {
    let Some(args) = read_cmdline_args(pid) else {
        return false;
    };
    agent_doc_controller::command_line::same_project_controller_args_match_project_root(
        &args,
        project_root,
    )
}

pub fn controller_serve_project_root(pid: u32) -> Option<PathBuf> {
    agent_doc_controller::command_line::controller_serve_project_root_from_args(&read_cmdline_args(
        pid,
    )?)
}

pub fn cmdline_has_preparing_handoff(pid: u32) -> bool {
    let Some(args) = read_cmdline_args(pid) else {
        return false;
    };
    agent_doc_controller::command_line::args_have_preparing_handoff(&args)
}

pub fn is_preparing_handoff_successor_pid(
    project_root: &Path,
    pid: u32,
    previous_controller_pid: u32,
    generation: u64,
) -> bool {
    let Some(args) = read_cmdline_args(pid) else {
        return false;
    };
    agent_doc_controller::command_line::preparing_handoff_successor_args_match(
        &args,
        project_root,
        previous_controller_pid,
        generation,
    )
}

pub fn open_supervisor_document(pid: u32) -> Option<PathBuf> {
    supervisor_document_from_observation(&read_cmdline_args(pid)?, process_cwd(pid).as_deref())
}

fn process_cwd(pid: u32) -> Option<PathBuf> {
    std::fs::read_link(Path::new("/proc").join(pid.to_string()).join("cwd")).ok()
}

/// Resolve a document argument in the process namespace that authored it.
///
/// Supervisors commonly preserve the relative path passed from their
/// project shell (for example, `tasks/plan.md`). Install fan-out scans those
/// command lines from a different working directory, so resolving against the
/// installer's cwd silently targets the wrong document and leaves that supervisor
/// on the old binary.
fn resolve_process_document_path(document: &Path, process_cwd: Option<&Path>) -> PathBuf {
    if document.is_absolute() {
        return document.to_path_buf();
    }
    process_cwd
        .map(|cwd| cwd.join(document))
        .unwrap_or_else(|| document.to_path_buf())
}

fn supervisor_document_from_observation(
    args: &[String],
    process_cwd: Option<&Path>,
) -> Option<PathBuf> {
    let document = agent_doc_controller::command_line::start_supervisor_document_from_args(args)?;
    Some(resolve_process_document_path(&document, process_cwd))
}

pub fn project_controller_pids(project_root: &Path) -> Vec<u32> {
    process_pids()
        .into_iter()
        .filter(|pid| is_same_project_controller_pid(project_root, *pid))
        .collect()
}

pub fn controller_project_roots(exclude_pid: u32) -> BTreeSet<PathBuf> {
    process_pids()
        .into_iter()
        .filter(|pid| *pid != exclude_pid)
        .filter_map(controller_serve_project_root)
        .map(|root| {
            agent_doc_controller::command_line::canonical_path_for_command_line_compare(&root)
        })
        .collect()
}

pub fn open_supervisor_documents(exclude_pid: u32) -> BTreeSet<PathBuf> {
    process_pids()
        .into_iter()
        .filter(|pid| *pid != exclude_pid)
        .filter_map(open_supervisor_document)
        .map(|doc| {
            agent_doc_controller::command_line::canonical_path_for_command_line_compare(&doc)
        })
        .collect()
}

pub fn send_signal(pid: u32, signal: ProcessSignal) {
    let _ = Command::new("kill")
        .arg(signal.as_arg())
        .arg(pid.to_string())
        .status();
}

fn read_cmdline_args(pid: u32) -> Option<Vec<String>> {
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    Some(parse_cmdline_args(&cmdline))
}

fn parse_cmdline_args(cmdline: &[u8]) -> Vec<String> {
    cmdline
        .split(|byte| *byte == 0)
        .filter(|arg| !arg.is_empty())
        .map(|arg| String::from_utf8_lossy(arg).to_string())
        .collect()
}

fn system_boot_timestamp_secs_from_uptime(now_secs: u64, uptime: &str) -> Option<u64> {
    let uptime_secs = uptime.split_whitespace().next()?.parse::<f64>().ok()?;
    if !uptime_secs.is_finite() || uptime_secs.is_sign_negative() {
        return None;
    }
    Some(now_secs.saturating_sub(uptime_secs.floor() as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cmdline_args_splits_null_separated_args() {
        assert_eq!(
            parse_cmdline_args(b"agent-doc\0controller\0serve\0\0"),
            vec!["agent-doc", "controller", "serve"]
        );
    }

    #[test]
    fn uptime_parse_uses_first_field_and_rejects_invalid_values() {
        assert_eq!(
            system_boot_timestamp_secs_from_uptime(1_000, "12.99 1234.56"),
            Some(988)
        );
        assert_eq!(system_boot_timestamp_secs_from_uptime(1_000, "-1 0"), None);
        assert_eq!(system_boot_timestamp_secs_from_uptime(1_000, "nan 0"), None);
        assert_eq!(system_boot_timestamp_secs_from_uptime(1_000, ""), None);
    }

    // A `/proc/<pid>/stat` line with `comm` containing spaces and a `)`, state
    // `state`, and `starttime` (field 22) = `start`.
    fn stat_line(state: char, start: u64) -> String {
        let mut fields = vec![state.to_string()];
        fields.extend((4..=21).map(|field| field.to_string()));
        fields.push(start.to_string());
        fields.extend(["999", "888"].map(str::to_string));
        format!("4242 (agent doc) x) {}", fields.join(" "))
    }

    #[test]
    fn proc_stat_parse_counts_fields_from_the_last_paren() {
        assert_eq!(
            parse_proc_stat_state_and_start(&stat_line('S', 12_345)),
            Some(('S', 12_345))
        );
        assert_eq!(parse_proc_stat_state_and_start("garbage"), None);
    }

    #[test]
    fn recorded_process_alive_rejects_pid_reuse_and_zombies() {
        let boot = 1_000_000;
        let ticks = Some(100);
        // Started 50s after boot, claim recorded 100s after boot: the owner.
        assert!(recorded_process_alive_from_stat(
            &stat_line('S', 5_000),
            ticks,
            Some(boot),
            boot + 100
        ));
        // Started 500s after boot, after the claim: a recycled pid.
        assert!(!recorded_process_alive_from_stat(
            &stat_line('S', 50_000),
            ticks,
            Some(boot),
            boot + 100
        ));
        // Within boot-time jitter of the claim: still the owner.
        assert!(recorded_process_alive_from_stat(
            &stat_line('R', 10_200),
            ticks,
            Some(boot),
            boot + 100
        ));
        // A zombie has exited even though its pid is still listed.
        assert!(!recorded_process_alive_from_stat(
            &stat_line('Z', 5_000),
            ticks,
            Some(boot),
            boot + 100
        ));
        // Unknown clock: never claim death that cannot be proven.
        assert!(recorded_process_alive_from_stat(
            &stat_line('S', 50_000),
            None,
            Some(boot),
            boot + 100
        ));
    }

    #[test]
    fn recorded_process_is_alive_for_self_and_dead_for_missing_pid() {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(recorded_process_is_alive(std::process::id(), now));
        assert!(!recorded_process_is_alive(u32::MAX - 2, now));
        assert!(!recorded_process_is_alive(0, now));
        // This process started long before the epoch; a claim recorded that
        // early cannot belong to it.
        #[cfg(target_os = "linux")]
        assert!(!recorded_process_is_alive(std::process::id(), 10));
    }

    #[test]
    fn signal_args_match_kill_flags() {
        assert_eq!(ProcessSignal::Term.as_arg(), "-TERM");
        assert_eq!(ProcessSignal::Kill.as_arg(), "-KILL");
    }

    #[test]
    fn supervisor_relative_documents_resolve_against_each_supervisor_cwd() {
        let first_cwd = Path::new("workspace-one");
        let second_cwd = Path::new("workspace-two");
        let relative = Path::new("tasks/plan.md");

        assert_eq!(
            resolve_process_document_path(relative, Some(first_cwd)),
            first_cwd.join(relative)
        );
        assert_eq!(
            resolve_process_document_path(relative, Some(second_cwd)),
            second_cwd.join(relative)
        );
        assert_ne!(
            resolve_process_document_path(relative, Some(first_cwd)),
            resolve_process_document_path(relative, Some(second_cwd)),
            "install fan-out must not collapse supervisors from different project cwd values"
        );
    }

    #[test]
    fn supervisor_document_resolution_preserves_absolute_and_missing_cwd_paths() {
        let absolute = std::env::temp_dir().join("sample-app/tasks/plan.md");
        assert_eq!(
            resolve_process_document_path(&absolute, Some(Path::new("ignored"))),
            absolute
        );

        let relative = Path::new("tasks/plan.md");
        assert_eq!(
            resolve_process_document_path(relative, None),
            relative.to_path_buf()
        );
    }

    #[test]
    fn install_fanout_discovers_route_owned_and_operator_started_supervisors() {
        let cwd = Path::new("sample-workspace");
        let route_owned = vec![
            "/bin/agent-doc".to_string(),
            "start".to_string(),
            "--route-owned".to_string(),
            "tasks/route-owned.md".to_string(),
        ];
        let operator_started = vec![
            "/bin/agent-doc".to_string(),
            "start".to_string(),
            "--resume".to_string(),
            "--".to_string(),
            "tasks/operator-started.md".to_string(),
        ];

        assert_eq!(
            supervisor_document_from_observation(&route_owned, Some(cwd)),
            Some(cwd.join("tasks/route-owned.md"))
        );
        assert_eq!(
            supervisor_document_from_observation(&operator_started, Some(cwd)),
            Some(cwd.join("tasks/operator-started.md"))
        );
    }
}
