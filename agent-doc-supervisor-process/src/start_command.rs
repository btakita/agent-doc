//! Supervisor start command rendering.
//!
//! Process ownership lives in this crate. Callers provide already-resolved
//! binary and document facts; this module only renders the shell command to
//! submit into a preserved route-owned pane.

use std::path::{Path, PathBuf};

use agent_doc_harness::ResumeRequest;
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

/// Everything that shapes a route-owned `agent-doc start` command line.
///
/// `#restartresume`: this replaced a chain of `_with_reap_policy`,
/// `_with_stderr_log`, `_with_reap_policy_and_stderr_log` overloads. Adding
/// `resume` as one more positional bool through five functions already carrying
/// `#[allow(clippy::too_many_arguments)]` is how a "which bool was that?" bug
/// gets written, and a silently-wrong resume flag starts a FRESH conversation
/// instead of erroring — the failure is invisible until the context is gone.
#[derive(Debug, Clone)]
pub struct RouteOwnedStartOptions<'a> {
    pub reap_policy: RouteOwnedReapPolicy,
    /// Append `2>> <path>` so route-owned boot stderr cannot bleed into the
    /// agent pane (`#restartstderrbleed`).
    pub stderr_log: Option<&'a Path>,
    /// Resume the harness conversation instead of starting a fresh one.
    ///
    /// A supervisor restart that escalates to a cold start must still honour
    /// `restart-supervisor`'s default continue-mode: the operator asked to
    /// restart the *process*, not to discard the conversation.
    pub resume: Option<ResumeRequest>,
}

impl RouteOwnedStartOptions<'_> {
    pub fn new(reap_policy: RouteOwnedReapPolicy) -> Self {
        Self {
            reap_policy,
            stderr_log: None,
            resume: None,
        }
    }
}

impl Default for RouteOwnedStartOptions<'_> {
    fn default() -> Self {
        Self::new(RouteOwnedReapPolicy::Auto)
    }
}

/// Build the route-owned `agent-doc start` command line.
pub fn route_owned_start_command_with_options(
    agent_doc_bin: &str,
    file: &Path,
    options: &RouteOwnedStartOptions<'_>,
) -> String {
    let mut cmd = format!(
        "{} start --route-owned --route-owned-reap-policy {}",
        shell_quote_arg(agent_doc_bin),
        options.reap_policy,
    );
    // `--resume` takes an OPTIONAL value (`num_args = 0..=1`), so a bare
    // `--resume <file>` makes clap swallow the document path as the resume ID and
    // then reject the command for a missing `<FILE>`. Use the `=` form for an id
    // and always close the flag with `--` before the positional, so the document
    // path can never be read as a value. Verified against the real binary.
    match &options.resume {
        Some(ResumeRequest::Latest) => cmd.push_str(" --resume --"),
        Some(ResumeRequest::Id(id)) => {
            cmd.push_str(" --resume=");
            cmd.push_str(&shell_quote_arg(id));
            cmd.push_str(" --");
        }
        None => {}
    }
    cmd.push(' ');
    cmd.push_str(&shell_quote_arg(&file.to_string_lossy()));
    if let Some(stderr_log) = options.stderr_log {
        cmd = format!(
            "{cmd} 2>> {}",
            shell_quote_arg(&stderr_log.to_string_lossy())
        );
    }
    cmd
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
    route_owned_start_command_with_options(
        agent_doc_bin,
        file,
        &RouteOwnedStartOptions::new(reap_policy),
    )
}

pub fn route_owned_start_command_with_reap_policy_and_stderr_log(
    agent_doc_bin: &str,
    file: &Path,
    reap_policy: RouteOwnedReapPolicy,
    stderr_log: &Path,
) -> String {
    route_owned_start_command_with_options(
        agent_doc_bin,
        file,
        &RouteOwnedStartOptions {
            reap_policy,
            stderr_log: Some(stderr_log),
            resume: None,
        },
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

    /// `#restartresume`: the resume flag must land BEFORE the positional file
    /// argument. Emitting `… <file> --resume` makes clap read the file as the
    /// resume ID and the flag as the file, which silently starts a fresh
    /// conversation on a bogus path instead of erroring.
    #[test]
    fn route_owned_start_command_places_resume_before_the_file_argument() {
        assert_eq!(
            route_owned_start_command_with_options(
                "/usr/local/bin/agent-doc",
                Path::new("tasks/doc.md"),
                &RouteOwnedStartOptions {
                    reap_policy: RouteOwnedReapPolicy::Auto,
                    stderr_log: None,
                    resume: Some(ResumeRequest::Latest),
                },
            ),
            "/usr/local/bin/agent-doc start --route-owned --route-owned-reap-policy auto --resume -- tasks/doc.md"
        );
    }

    /// An id-addressed resume passes the id through the same quoting as every
    /// other argument — a conversation id with a space would otherwise split
    /// into two argv entries and resume nothing.
    #[test]
    fn route_owned_start_command_quotes_resume_id() {
        assert_eq!(
            route_owned_start_command_with_options(
                "agent-doc",
                Path::new("tasks/doc.md"),
                &RouteOwnedStartOptions {
                    reap_policy: RouteOwnedReapPolicy::Auto,
                    stderr_log: None,
                    resume: Some(ResumeRequest::Id("conv 42".into())),
                },
            ),
            "agent-doc start --route-owned --route-owned-reap-policy auto --resume='conv 42' -- tasks/doc.md"
        );
    }

    /// The stderr redirect must stay the OUTERMOST element (`#restartstderrbleed`):
    /// route-owned boot stderr has to be captured before the binary runs, so
    /// `2>>` cannot end up between the flags and the file.
    #[test]
    fn route_owned_start_command_keeps_stderr_redirect_outermost_with_resume() {
        assert_eq!(
            route_owned_start_command_with_options(
                "agent-doc",
                Path::new("tasks/doc.md"),
                &RouteOwnedStartOptions {
                    reap_policy: RouteOwnedReapPolicy::Auto,
                    stderr_log: Some(Path::new("/tmp/supervisor-stderr.log")),
                    resume: Some(ResumeRequest::Latest),
                },
            ),
            "agent-doc start --route-owned --route-owned-reap-policy auto --resume -- tasks/doc.md 2>> /tmp/supervisor-stderr.log"
        );
    }

    /// No resume intent must render exactly the pre-existing command, so the
    /// options struct is a pure refactor for every existing caller.
    #[test]
    fn route_owned_start_command_without_resume_matches_legacy_rendering() {
        let legacy = route_owned_start_command_with_reap_policy_and_stderr_log(
            "agent-doc",
            Path::new("tasks/doc.md"),
            RouteOwnedReapPolicy::Auto,
            Path::new("/tmp/err.log"),
        );
        assert_eq!(
            legacy,
            "agent-doc start --route-owned --route-owned-reap-policy auto tasks/doc.md 2>> /tmp/err.log"
        );
        assert!(!legacy.contains("--resume"));
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
