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
