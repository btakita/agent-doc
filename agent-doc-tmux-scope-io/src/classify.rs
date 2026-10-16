//! Pure classification of `tmux-spawn-*.scope` units (`#tmuxscopesteward`).
//!
//! Everything here is a function of observations the IO layer hands in
//! (`systemctl show` text, `/proc` facts, path existence), so the stewardship
//! policy is testable against fixture output without a systemd user manager.
//!
//! The policy, in order:
//!
//! 1. A `failed` scope is debris: tmux asked systemd to move a pane into a new
//!    scope, the pane exited first ("No PIDs left to attach to the scope's
//!    control group, refusing."), and the failed transient unit stays loaded
//!    until `reset-failed`. Resetting it touches no process.
//! 2. A running scope whose launching tmux server is alive belongs to that
//!    server. Keep it.
//! 3. A running scope is orphaned when its launching server is gone, or when
//!    the pane's own process has left it (the pane closed but a daemon it
//!    spawned still pins the scope). An orphaned scope is stopped ONLY when every
//!    process left in it is provably an agent-doc test-fixture process (an
//!    `agent-doc` binary rooted in a `tempfile` `.tmp*` directory) that is either
//!    old enough or whose fixture root is already gone. Anything else is
//!    reported and left alone: an operator process is never killed.

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Unit-name prefix tmux (built with systemd support) uses for pane scopes.
pub const TMUX_SPAWN_SCOPE_PREFIX: &str = "tmux-spawn-";
/// Unit-name suffix for transient scopes.
pub const SCOPE_SUFFIX: &str = ".scope";
/// The glob handed to `systemctl` so it only ever touches tmux pane scopes.
pub const TMUX_SPAWN_SCOPE_GLOB: &str = "tmux-spawn-*.scope";
/// `/proc/<pid>/comm` of a tmux server process.
pub const TMUX_SERVER_COMM: &str = "tmux: server";

/// One unit block parsed from `systemctl show -p Id,Description,...`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ScopeUnit {
    pub id: String,
    pub description: String,
    pub load_state: String,
    pub active_state: String,
    pub sub_state: String,
    pub result: String,
    pub control_group: String,
}

impl ScopeUnit {
    pub fn is_failed(&self) -> bool {
        self.active_state == "failed"
    }

    pub fn is_running(&self) -> bool {
        matches!(
            self.active_state.as_str(),
            "active" | "activating" | "reloading"
        )
    }

    /// `(pane_pid, launcher_pid)` from tmux's
    /// `tmux child pane <pid> launched by process <pid>` description.
    pub fn pids(&self) -> Option<(u32, u32)> {
        parse_scope_description(&self.description)
    }
}

/// True for unit names tmux creates for its panes.
pub fn is_tmux_spawn_scope(unit: &str) -> bool {
    unit.starts_with(TMUX_SPAWN_SCOPE_PREFIX)
        && unit.ends_with(SCOPE_SUFFIX)
        && unit.len() > TMUX_SPAWN_SCOPE_PREFIX.len() + SCOPE_SUFFIX.len()
}

/// Parse the blank-line-separated `Key=Value` blocks `systemctl show` prints.
///
/// Units that are not tmux pane scopes are dropped, so a glob that matched
/// something unexpected can never widen what the steward acts on.
pub fn parse_systemctl_show(text: &str) -> Vec<ScopeUnit> {
    let mut units = Vec::new();
    let mut current = ScopeUnit::default();
    let mut has_fields = false;
    let mut flush = |unit: &mut ScopeUnit, has_fields: &mut bool| {
        if *has_fields && is_tmux_spawn_scope(&unit.id) {
            units.push(std::mem::take(unit));
        } else {
            *unit = ScopeUnit::default();
        }
        *has_fields = false;
    };
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() {
            flush(&mut current, &mut has_fields);
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        has_fields = true;
        let value = value.to_string();
        match key {
            "Id" => current.id = value,
            "Description" => current.description = value,
            "LoadState" => current.load_state = value,
            "ActiveState" => current.active_state = value,
            "SubState" => current.sub_state = value,
            "Result" => current.result = value,
            "ControlGroup" => current.control_group = value,
            _ => {}
        }
    }
    flush(&mut current, &mut has_fields);
    units
}

/// Parse `tmux child pane <pane_pid> launched by process <launcher_pid>`.
pub fn parse_scope_description(description: &str) -> Option<(u32, u32)> {
    let rest = description.trim().strip_prefix("tmux child pane ")?;
    let (pane, rest) = rest.split_once(' ')?;
    let launcher = rest.strip_prefix("launched by process ")?;
    Some((pane.parse().ok()?, launcher.trim().parse().ok()?))
}

/// Whether the tmux server that launched a scope still exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LauncherState {
    /// `/proc/<pid>` is a live tmux server.
    Alive,
    /// No such pid, or the pid now names something other than a tmux server.
    Gone,
    /// The description did not carry a launcher pid.
    Unknown,
}

/// Classify a launcher from what `/proc/<pid>/comm` said (`None` = no process).
pub fn launcher_state_from_comm(comm: Option<&str>) -> LauncherState {
    match comm.map(str::trim) {
        Some(TMUX_SERVER_COMM) => LauncherState::Alive,
        Some(_) | None => LauncherState::Gone,
    }
}

/// One process observed inside a scope's cgroup.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ScopeProcess {
    pub pid: u32,
    pub argv: Vec<String>,
    /// Basename of `/proc/<pid>/exe` (`" (deleted)"` stripped), when readable.
    pub exe_name: Option<String>,
    pub cwd: Option<PathBuf>,
    /// Seconds since the process started, when observable.
    pub age_secs: Option<u64>,
}

/// Everything the steward knows about one scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeObservation {
    pub unit: ScopeUnit,
    pub launcher: LauncherState,
    /// `None` when the cgroup's process list could not be read.
    pub processes: Option<Vec<ScopeProcess>>,
}

/// Tunables for [`classify_scope`].
#[derive(Debug, Clone)]
pub struct ClassifyPolicy {
    /// Temp bases whose `.tmp*` children are `tempfile` fixture roots.
    pub temp_bases: Vec<PathBuf>,
    /// A fixture process whose root still exists must be at least this old
    /// before it is stopped, so a test that is still running is never cut off.
    pub min_leak_age: Duration,
}

impl ClassifyPolicy {
    pub fn with_temp_bases(extra_temp_base: &Path, min_leak_age: Duration) -> Self {
        let mut temp_bases = vec![
            PathBuf::from("/tmp"),
            PathBuf::from("/var/tmp"),
            PathBuf::from("/dev/shm"),
        ];
        if extra_temp_base.is_absolute()
            && extra_temp_base != Path::new("/")
            && !temp_bases.iter().any(|base| base == extra_temp_base)
        {
            temp_bases.push(extra_temp_base.to_path_buf());
        }
        Self {
            temp_bases,
            min_leak_age,
        }
    }
}

/// What the steward does with one scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum ScopeVerdict {
    /// Failed transient unit: `reset-failed` unloads it.
    ResetFailed,
    /// Owned by a live tmux server (or not in a state the steward acts on).
    Keep { reason: String },
    /// Orphaned scope holding only agent-doc fixture leaks: stop it.
    StopLeak { reason: String, pids: Vec<u32> },
    /// Orphaned scope the steward cannot prove is agent-doc's: report only.
    Report { reason: String, pids: Vec<u32> },
}

/// Is `path` inside a `tempfile` directory (`<temp>/.tmpXXXX/...`)?
pub fn is_temp_fixture_path(path: &Path, temp_bases: &[PathBuf]) -> bool {
    temp_bases
        .iter()
        .filter(|base| base.is_absolute() && base.as_path() != Path::new("/"))
        .any(|base| {
            path.strip_prefix(base)
                .ok()
                .and_then(|rest| rest.components().next())
                .and_then(|first| first.as_os_str().to_str())
                .is_some_and(|name| name.starts_with(".tmp"))
        })
}

/// The `<temp>/.tmpXXXX` directory a fixture path lives in.
pub fn temp_fixture_root(path: &Path, temp_bases: &[PathBuf]) -> Option<PathBuf> {
    temp_bases.iter().find_map(|base| {
        let rest = path.strip_prefix(base).ok()?;
        let first = rest.components().next()?.as_os_str().to_str()?;
        first.starts_with(".tmp").then(|| base.join(first))
    })
}

fn basename(value: &str) -> &str {
    value.rsplit('/').next().unwrap_or(value)
}

/// True when the process is the `agent-doc` binary (by argv[0] or exe name).
pub fn is_agent_doc_binary(process: &ScopeProcess) -> bool {
    let argv0 = process.argv.first().map(|arg| basename(arg));
    argv0 == Some("agent-doc") || process.exe_name.as_deref() == Some("agent-doc")
}

/// The absolute paths a process names: absolute argv tokens, including the
/// value half of `--flag=/path`. argv[0] is the binary, not an operand.
fn absolute_path_operands(process: &ScopeProcess) -> Vec<PathBuf> {
    process
        .argv
        .iter()
        .skip(1)
        .filter_map(|arg| {
            let candidate = arg
                .strip_prefix("--")
                .and_then(|flag| flag.split_once('='))
                .map(|(_, value)| value)
                .unwrap_or(arg);
            candidate.starts_with('/').then(|| PathBuf::from(candidate))
        })
        .collect()
}

/// The fixture roots that prove a process is an agent-doc test fixture.
///
/// Proof requires the agent-doc binary AND that every absolute path it names
/// lies in a temp fixture root (or, naming none, that its cwd does). A single
/// non-temp path operand (a real project, a real document) disproves it.
pub fn agent_doc_fixture_roots(
    process: &ScopeProcess,
    temp_bases: &[PathBuf],
) -> Option<Vec<PathBuf>> {
    if !is_agent_doc_binary(process) {
        return None;
    }
    let operands = absolute_path_operands(process);
    let evidence: Vec<PathBuf> = if operands.is_empty() {
        vec![process.cwd.clone()?]
    } else {
        operands
    };
    let mut roots = Vec::new();
    for path in &evidence {
        let root = temp_fixture_root(path, temp_bases)?;
        if !roots.contains(&root) {
            roots.push(root);
        }
    }
    Some(roots)
}

/// Decide what to do with one scope. `path_exists` is the only IO edge.
pub fn classify_scope(
    observation: &ScopeObservation,
    policy: &ClassifyPolicy,
    path_exists: &dyn Fn(&Path) -> bool,
) -> ScopeVerdict {
    let unit = &observation.unit;
    if unit.is_failed() {
        return ScopeVerdict::ResetFailed;
    }
    if !unit.is_running() {
        return ScopeVerdict::Keep {
            reason: format!("state_{}", unit.active_state),
        };
    }
    let pane_pid = unit.pids().map(|(pane, _)| pane);
    let orphan_cause = match observation.launcher {
        LauncherState::Unknown => {
            return ScopeVerdict::Report {
                reason: "launcher_unknown".to_string(),
                pids: observed_pids(observation),
            };
        }
        LauncherState::Alive => {
            // The pane is alive while its root process is still in the scope.
            // A pane that closed while a daemon it spawned lives on leaves the
            // scope running under a live server: that is orphaned too.
            match (observation.processes.as_ref(), pane_pid) {
                (Some(processes), Some(pane))
                    if !processes.iter().any(|process| process.pid == pane) =>
                {
                    "pane_gone"
                }
                _ => {
                    return ScopeVerdict::Keep {
                        reason: "launcher_alive".to_string(),
                    };
                }
            }
        }
        LauncherState::Gone => "launcher_gone",
    };
    let Some(processes) = observation.processes.as_ref() else {
        return ScopeVerdict::Report {
            reason: format!("{orphan_cause}_processes_unreadable"),
            pids: Vec::new(),
        };
    };
    if processes.is_empty() {
        // tmux blocks on the systemd job while it moves a pane, so a pane that
        // exits instantly is still an unreaped zombie when systemd attaches it.
        // The scope then reads `active` with an empty cgroup for good. Stopping
        // it signals no process.
        if unit.active_state == "active" {
            return ScopeVerdict::StopLeak {
                reason: format!("{orphan_cause}_empty_scope"),
                pids: Vec::new(),
            };
        }
        return ScopeVerdict::Report {
            reason: format!("{orphan_cause}_no_processes"),
            pids: Vec::new(),
        };
    }
    let pids: Vec<u32> = processes.iter().map(|process| process.pid).collect();
    for process in processes {
        let Some(roots) = agent_doc_fixture_roots(process, &policy.temp_bases) else {
            return ScopeVerdict::Report {
                reason: format!("{orphan_cause}_foreign_process pid={}", process.pid),
                pids,
            };
        };
        let root_gone = roots.iter().any(|root| !path_exists(root));
        let old_enough = process
            .age_secs
            .is_some_and(|age| age >= policy.min_leak_age.as_secs());
        if !root_gone && !old_enough {
            return ScopeVerdict::Report {
                reason: format!("{orphan_cause}_fixture_too_young pid={}", process.pid),
                pids,
            };
        }
    }
    ScopeVerdict::StopLeak {
        reason: format!("{orphan_cause}_agent_doc_fixture_only"),
        pids,
    }
}

fn observed_pids(observation: &ScopeObservation) -> Vec<u32> {
    observation
        .processes
        .as_ref()
        .map(|processes| processes.iter().map(|process| process.pid).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHOW_FIXTURE: &str = "\
Id=tmux-spawn-142cd867-e153-483d-a555-59bbe770b0b6.scope
Description=tmux child pane 212902 launched by process 7004
LoadState=loaded
ActiveState=active
SubState=running
Result=success
ControlGroup=/user.slice/user-1000.slice/user@1000.service/tmux-spawn-142cd867-e153-483d-a555-59bbe770b0b6.scope

Id=tmux-spawn-243864e8-0934-42ba-907e-6d68b6a2948d.scope
Description=tmux child pane 1545397 launched by process 1545380
LoadState=loaded
ActiveState=failed
SubState=failed
Result=resources
ControlGroup=

Id=kitty-6968-0.scope
Description=kitty
LoadState=loaded
ActiveState=active
SubState=running
Result=success
ControlGroup=/user.slice/user-1000.slice/user@1000.service/kitty-6968-0.scope
";

    fn policy() -> ClassifyPolicy {
        ClassifyPolicy::with_temp_bases(Path::new("/tmp"), Duration::from_secs(600))
    }

    fn running(launcher: LauncherState, processes: Option<Vec<ScopeProcess>>) -> ScopeObservation {
        ScopeObservation {
            unit: ScopeUnit {
                id: "tmux-spawn-aaaa.scope".to_string(),
                description: "tmux child pane 10 launched by process 9".to_string(),
                load_state: "loaded".to_string(),
                active_state: "active".to_string(),
                sub_state: "running".to_string(),
                result: "success".to_string(),
                control_group: "/user.slice/tmux-spawn-aaaa.scope".to_string(),
            },
            launcher,
            processes,
        }
    }

    fn controller(pid: u32, root: &str, age_secs: u64) -> ScopeProcess {
        ScopeProcess {
            pid,
            argv: vec![
                "/home/u/agent-doc/target/debug/agent-doc".to_string(),
                "controller".to_string(),
                "serve".to_string(),
                "--project-root".to_string(),
                root.to_string(),
            ],
            exe_name: Some("agent-doc".to_string()),
            cwd: Some(PathBuf::from("/")),
            age_secs: Some(age_secs),
        }
    }

    fn pane_shell(pid: u32) -> ScopeProcess {
        ScopeProcess {
            pid,
            argv: vec!["-zsh".to_string()],
            exe_name: Some("zsh".to_string()),
            cwd: Some(PathBuf::from("/home/u")),
            age_secs: Some(99_999),
        }
    }

    fn exists_none(_: &Path) -> bool {
        false
    }

    fn exists_all(_: &Path) -> bool {
        true
    }

    #[test]
    fn parse_show_keeps_only_tmux_spawn_scopes() {
        let units = parse_systemctl_show(SHOW_FIXTURE);
        assert_eq!(units.len(), 2);
        assert_eq!(units[0].pids(), Some((212902, 7004)));
        assert!(units[0].is_running());
        assert!(units[1].is_failed());
        assert_eq!(units[1].result, "resources");
        assert_eq!(units[1].pids(), Some((1545397, 1545380)));
    }

    #[test]
    fn parse_description_rejects_foreign_text() {
        assert_eq!(parse_scope_description("kitty"), None);
        assert_eq!(
            parse_scope_description("tmux child pane x launched by process 1"),
            None
        );
        assert_eq!(
            parse_scope_description("tmux child pane 5 launched by process 6"),
            Some((5, 6))
        );
    }

    #[test]
    fn unit_name_filter_requires_prefix_and_suffix() {
        assert!(is_tmux_spawn_scope("tmux-spawn-1.scope"));
        assert!(!is_tmux_spawn_scope("tmux-spawn-.scope"));
        assert!(!is_tmux_spawn_scope("tmux-spawn-1.service"));
        assert!(!is_tmux_spawn_scope("app-tmux-spawn-1.scope"));
    }

    #[test]
    fn launcher_comm_must_be_a_tmux_server() {
        assert_eq!(
            launcher_state_from_comm(Some("tmux: server\n")),
            LauncherState::Alive
        );
        assert_eq!(launcher_state_from_comm(Some("bash")), LauncherState::Gone);
        assert_eq!(launcher_state_from_comm(None), LauncherState::Gone);
    }

    #[test]
    fn failed_scope_is_reset_regardless_of_launcher() {
        let mut observation = running(LauncherState::Alive, None);
        observation.unit.active_state = "failed".to_string();
        assert_eq!(
            classify_scope(&observation, &policy(), &exists_all),
            ScopeVerdict::ResetFailed
        );
    }

    #[test]
    fn live_launcher_scope_is_kept_even_with_fixture_processes() {
        let observation = running(
            LauncherState::Alive,
            Some(vec![
                pane_shell(10),
                controller(11, "/tmp/.tmpAbc/project", 99_999),
            ]),
        );
        assert!(matches!(
            classify_scope(&observation, &policy(), &exists_none),
            ScopeVerdict::Keep { .. }
        ));
    }

    #[test]
    fn orphaned_fixture_controller_with_removed_root_is_stopped() {
        let observation = running(
            LauncherState::Gone,
            Some(vec![controller(11, "/tmp/.tmpAbc/project", 5)]),
        );
        assert_eq!(
            classify_scope(&observation, &policy(), &exists_none),
            ScopeVerdict::StopLeak {
                reason: "launcher_gone_agent_doc_fixture_only".to_string(),
                pids: vec![11],
            }
        );
    }

    #[test]
    fn orphaned_fixture_controller_old_enough_is_stopped_even_if_root_exists() {
        let observation = running(
            LauncherState::Gone,
            Some(vec![controller(11, "/tmp/.tmpAbc", 601)]),
        );
        assert!(matches!(
            classify_scope(&observation, &policy(), &exists_all),
            ScopeVerdict::StopLeak { .. }
        ));
    }

    #[test]
    fn young_fixture_with_live_root_is_only_reported() {
        let observation = running(
            LauncherState::Gone,
            Some(vec![controller(11, "/tmp/.tmpAbc", 30)]),
        );
        assert!(matches!(
            classify_scope(&observation, &policy(), &exists_all),
            ScopeVerdict::Report { reason, .. } if reason.starts_with("launcher_gone_fixture_too_young")
        ));
    }

    #[test]
    fn operator_process_in_orphaned_scope_is_never_stopped() {
        let shell = ScopeProcess {
            pid: 12,
            argv: vec!["-zsh".to_string()],
            exe_name: Some("zsh".to_string()),
            cwd: Some(PathBuf::from("/tmp/.tmpAbc")),
            age_secs: Some(99_999),
        };
        let observation = running(
            LauncherState::Gone,
            Some(vec![controller(11, "/tmp/.tmpAbc", 99_999), shell]),
        );
        assert!(matches!(
            classify_scope(&observation, &policy(), &exists_none),
            ScopeVerdict::Report { reason, pids } if reason == "launcher_gone_foreign_process pid=12" && pids == vec![11, 12]
        ));
    }

    #[test]
    fn agent_doc_on_a_real_project_is_never_stopped() {
        let observation = running(
            LauncherState::Gone,
            Some(vec![controller(11, "/home/u/work/project", 99_999)]),
        );
        assert!(matches!(
            classify_scope(&observation, &policy(), &exists_none),
            ScopeVerdict::Report { .. }
        ));
    }

    #[test]
    fn mixed_path_operands_disprove_fixture_ownership() {
        let mut process = controller(11, "/tmp/.tmpAbc", 99_999);
        process.argv.push("/home/u/work/plan.md".to_string());
        assert_eq!(
            agent_doc_fixture_roots(&process, &policy().temp_bases),
            None
        );
    }

    #[test]
    fn non_tempfile_temp_dirs_are_not_fixtures() {
        let observation = running(
            LauncherState::Gone,
            Some(vec![controller(
                11,
                "/tmp/agent-doc-native-77/project",
                99_999,
            )]),
        );
        assert!(matches!(
            classify_scope(&observation, &policy(), &exists_none),
            ScopeVerdict::Report { .. }
        ));
    }

    #[test]
    fn flag_equals_paths_and_cwd_are_evidence() {
        let mut process = controller(11, "/tmp/.tmpAbc", 1);
        process.argv = vec![
            "agent-doc".to_string(),
            "start".to_string(),
            "--project-root=/tmp/.tmpQ/p".to_string(),
        ];
        assert_eq!(
            agent_doc_fixture_roots(&process, &policy().temp_bases),
            Some(vec![PathBuf::from("/tmp/.tmpQ")])
        );
        process.argv = vec!["agent-doc".to_string(), "start".to_string()];
        process.cwd = Some(PathBuf::from("/tmp/.tmpZ/doc"));
        assert_eq!(
            agent_doc_fixture_roots(&process, &policy().temp_bases),
            Some(vec![PathBuf::from("/tmp/.tmpZ")])
        );
        process.cwd = None;
        assert_eq!(
            agent_doc_fixture_roots(&process, &policy().temp_bases),
            None
        );
    }

    #[test]
    fn unreadable_or_empty_cgroup_is_reported() {
        assert!(matches!(
            classify_scope(&running(LauncherState::Gone, None), &policy(), &exists_none),
            ScopeVerdict::Report { reason, .. } if reason == "launcher_gone_processes_unreadable"
        ));
        let mut activating = running(LauncherState::Gone, Some(Vec::new()));
        activating.unit.active_state = "activating".to_string();
        assert!(matches!(
            classify_scope(&activating, &policy(), &exists_none),
            ScopeVerdict::Report { reason, .. } if reason == "launcher_gone_no_processes"
        ));
        assert!(matches!(
            classify_scope(&running(LauncherState::Unknown, Some(Vec::new())), &policy(), &exists_none),
            ScopeVerdict::Report { reason, .. } if reason == "launcher_unknown"
        ));
    }

    #[test]
    fn inactive_loaded_scope_is_kept() {
        let mut observation = running(LauncherState::Gone, None);
        observation.unit.active_state = "inactive".to_string();
        assert!(matches!(
            classify_scope(&observation, &policy(), &exists_none),
            ScopeVerdict::Keep { reason } if reason == "state_inactive"
        ));
    }

    #[test]
    fn live_launcher_with_live_pane_is_kept() {
        let mut pane = controller(10, "/tmp/.tmpAbc", 99_999);
        pane.argv = vec!["-zsh".to_string()];
        let observation = running(LauncherState::Alive, Some(vec![pane]));
        assert!(matches!(
            classify_scope(&observation, &policy(), &exists_none),
            ScopeVerdict::Keep { reason } if reason == "launcher_alive"
        ));
    }

    #[test]
    fn closed_pane_leaving_only_a_fixture_daemon_is_stopped_under_a_live_server() {
        // Pane pid 10 is gone; only the daemonized fixture controller remains.
        let observation = running(
            LauncherState::Alive,
            Some(vec![controller(11, "/tmp/.tmpAbc/project", 5)]),
        );
        assert_eq!(
            classify_scope(&observation, &policy(), &exists_none),
            ScopeVerdict::StopLeak {
                reason: "pane_gone_agent_doc_fixture_only".to_string(),
                pids: vec![11],
            }
        );
    }

    #[test]
    fn closed_pane_leaving_an_operator_daemon_is_only_reported() {
        let job = ScopeProcess {
            pid: 11,
            argv: vec![
                "rsync".to_string(),
                "/home/u/a".to_string(),
                "/mnt/b".to_string(),
            ],
            exe_name: Some("rsync".to_string()),
            cwd: Some(PathBuf::from("/home/u")),
            age_secs: Some(99_999),
        };
        let observation = running(LauncherState::Alive, Some(vec![job]));
        assert!(matches!(
            classify_scope(&observation, &policy(), &exists_none),
            ScopeVerdict::Report { reason, .. } if reason == "pane_gone_foreign_process pid=11"
        ));
    }

    #[test]
    fn live_launcher_with_unreadable_cgroup_is_kept() {
        let observation = running(LauncherState::Alive, None);
        assert!(matches!(
            classify_scope(&observation, &policy(), &exists_none),
            ScopeVerdict::Keep { .. }
        ));
    }

    #[test]
    fn empty_active_scope_is_stopped_whether_launcher_lives_or_not() {
        assert_eq!(
            classify_scope(
                &running(LauncherState::Gone, Some(Vec::new())),
                &policy(),
                &exists_all
            ),
            ScopeVerdict::StopLeak {
                reason: "launcher_gone_empty_scope".to_string(),
                pids: Vec::new(),
            }
        );
        assert_eq!(
            classify_scope(
                &running(LauncherState::Alive, Some(Vec::new())),
                &policy(),
                &exists_all
            ),
            ScopeVerdict::StopLeak {
                reason: "pane_gone_empty_scope".to_string(),
                pids: Vec::new(),
            }
        );
    }
}
