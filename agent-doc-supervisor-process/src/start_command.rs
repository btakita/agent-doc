//! Supervisor start command rendering.
//!
//! Process ownership lives in this crate. Callers provide already-resolved
//! binary and document facts; this module only renders the shell command to
//! submit into a preserved route-owned pane.

use std::path::{Path, PathBuf};

use agent_doc_supervisor::route_owned::RouteOwnedReapPolicy;

fn shell_quote_arg(raw: &str) -> String {
    if !raw.is_empty()
        && raw
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | ':' | '+'))
    {
        return raw.to_string();
    }
    format!("'{}'", raw.replace('\'', "'\\''"))
}

pub fn route_owned_stderr_log_path(project_root: &Path) -> PathBuf {
    project_root
        .join(".agent-doc")
        .join("logs")
        .join("supervisor-stderr.log")
}

pub fn route_owned_start_command(agent_doc_bin: &str, file: &Path) -> String {
    route_owned_start_command_with_reap_policy(agent_doc_bin, file, RouteOwnedReapPolicy::Auto)
}

pub fn route_owned_start_command_with_stderr_log(
    agent_doc_bin: &str,
    file: &Path,
    stderr_log: &Path,
) -> String {
    route_owned_start_command_with_reap_policy_and_stderr_log(
        agent_doc_bin,
        file,
        RouteOwnedReapPolicy::Auto,
        stderr_log,
    )
}

pub fn route_owned_start_command_with_reap_policy(
    agent_doc_bin: &str,
    file: &Path,
    reap_policy: RouteOwnedReapPolicy,
) -> String {
    format!(
        "{} start --route-owned --route-owned-reap-policy {} {}",
        shell_quote_arg(agent_doc_bin),
        reap_policy,
        shell_quote_arg(&file.to_string_lossy())
    )
}

pub fn route_owned_start_command_with_reap_policy_and_stderr_log(
    agent_doc_bin: &str,
    file: &Path,
    reap_policy: RouteOwnedReapPolicy,
    stderr_log: &Path,
) -> String {
    format!(
        "{} 2>> {}",
        route_owned_start_command_with_reap_policy(agent_doc_bin, file, reap_policy),
        shell_quote_arg(&stderr_log.to_string_lossy()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_owned_start_command_preserves_safe_args_unquoted() {
        assert_eq!(
            route_owned_start_command("/usr/local/bin/agent-doc", Path::new("tasks/doc.md")),
            "/usr/local/bin/agent-doc start --route-owned --route-owned-reap-policy auto tasks/doc.md"
        );
    }

    #[test]
    fn route_owned_start_command_quotes_spaces() {
        assert_eq!(
            route_owned_start_command("/tmp/agent doc", Path::new("tasks/my doc.md")),
            "'/tmp/agent doc' start --route-owned --route-owned-reap-policy auto 'tasks/my doc.md'"
        );
    }

    #[test]
    fn route_owned_start_command_quotes_empty_args() {
        assert_eq!(
            route_owned_start_command("", Path::new("")),
            "'' start --route-owned --route-owned-reap-policy auto ''"
        );
    }

    #[test]
    fn route_owned_start_command_escapes_single_quotes() {
        assert_eq!(
            route_owned_start_command("/tmp/agent'doc", Path::new("tasks/brian's doc.md")),
            "'/tmp/agent'\\''doc' start --route-owned --route-owned-reap-policy auto 'tasks/brian'\\''s doc.md'"
        );
    }

    #[test]
    fn route_owned_start_command_renders_keep_alive_policy() {
        assert_eq!(
            route_owned_start_command_with_reap_policy(
                "/usr/local/bin/agent-doc",
                Path::new("tasks/doc.md"),
                RouteOwnedReapPolicy::KeepAlive,
            ),
            "/usr/local/bin/agent-doc start --route-owned --route-owned-reap-policy keep-alive tasks/doc.md"
        );
    }

    #[test]
    fn route_owned_start_command_quotes_boot_stderr_log() {
        assert_eq!(
            route_owned_start_command_with_stderr_log(
                "/usr/local/bin/agent-doc",
                Path::new("tasks/doc.md"),
                Path::new("/tmp/agent doc/supervisor-stderr.log"),
            ),
            "/usr/local/bin/agent-doc start --route-owned --route-owned-reap-policy auto tasks/doc.md 2>> '/tmp/agent doc/supervisor-stderr.log'"
        );
    }

    #[cfg(unix)]
    #[test]
    fn route_owned_start_command_keeps_boot_diagnostics_out_of_pane_streams() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let agent_doc = tmp.path().join("fake agent-doc");
        std::fs::write(
            &agent_doc,
            "#!/bin/sh\nprintf '%s\\n' '[start] boot diagnostic' >&2\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&agent_doc).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&agent_doc, permissions).unwrap();

        let stderr_log = route_owned_stderr_log_path(tmp.path());
        std::fs::create_dir_all(stderr_log.parent().unwrap()).unwrap();
        let command = route_owned_start_command_with_stderr_log(
            &agent_doc.to_string_lossy(),
            Path::new("tasks/doc.md"),
            &stderr_log,
        );
        let output = std::process::Command::new("/bin/sh")
            .args(["-c", &command])
            .output()
            .unwrap();

        assert!(output.status.success(), "{output:?}");
        assert!(output.stdout.is_empty(), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        assert_eq!(
            std::fs::read_to_string(stderr_log).unwrap(),
            "[start] boot diagnostic\n"
        );
    }
}
