//! Supervisor start command rendering.
//!
//! Process ownership lives in this crate. Callers provide already-resolved
//! binary and document facts; this module only renders the shell command to
//! submit into a preserved route-owned pane.

use std::path::Path;

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

/// Everything that shapes a route-owned `agent-doc start` command line.
///
/// `#restartresume`: keeping resume in one typed options struct avoids adding
/// positional booleans through a chain of overloads. A "which bool was that?" bug
/// gets written, and a silently-wrong resume flag starts a FRESH conversation
/// instead of erroring — the failure is invisible until the context is gone.
#[derive(Debug, Clone)]
pub struct RouteOwnedStartOptions {
    pub reap_policy: RouteOwnedReapPolicy,
    /// Resume the harness conversation instead of starting a fresh one.
    ///
    /// A supervisor restart that escalates to a cold start must still honour
    /// `restart-supervisor`'s default continue-mode: the operator asked to
    /// restart the *process*, not to discard the conversation.
    pub resume: Option<ResumeRequest>,
}

impl RouteOwnedStartOptions {
    pub fn new(reap_policy: RouteOwnedReapPolicy) -> Self {
        Self {
            reap_policy,
            resume: None,
        }
    }
}

impl Default for RouteOwnedStartOptions {
    fn default() -> Self {
        Self::new(RouteOwnedReapPolicy::Auto)
    }
}

/// Build the route-owned `agent-doc start` command line.
pub fn route_owned_start_command_with_options(
    agent_doc_bin: &str,
    file: &Path,
    options: &RouteOwnedStartOptions,
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
    cmd
}

pub fn route_owned_start_command(agent_doc_bin: &str, file: &Path) -> String {
    route_owned_start_command_with_reap_policy(agent_doc_bin, file, RouteOwnedReapPolicy::Auto)
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
                    resume: Some(ResumeRequest::Id("conv 42".into())),
                },
            ),
            "agent-doc start --route-owned --route-owned-reap-policy auto --resume='conv 42' -- tasks/doc.md"
        );
    }

    /// `agent-doc start` owns supervisor log setup. Its launch command must not
    /// depend on caller-owned shell redirection.
    #[test]
    fn route_owned_start_command_has_no_shell_stderr_redirect() {
        let command = route_owned_start_command_with_options(
            "agent-doc",
            Path::new("tasks/doc.md"),
            &RouteOwnedStartOptions {
                reap_policy: RouteOwnedReapPolicy::Auto,
                resume: Some(ResumeRequest::Latest),
            },
        );

        assert_eq!(
            command,
            "agent-doc start --route-owned --route-owned-reap-policy auto --resume -- tasks/doc.md"
        );
        assert!(!command.contains("2>>"));
    }

    /// No resume intent must render exactly the pre-existing command, so the
    /// options struct is a pure refactor for every existing caller.
    #[test]
    fn route_owned_start_command_without_resume_matches_legacy_rendering() {
        let command = route_owned_start_command_with_reap_policy(
            "agent-doc",
            Path::new("tasks/doc.md"),
            RouteOwnedReapPolicy::Auto,
        );
        assert_eq!(
            command,
            "agent-doc start --route-owned --route-owned-reap-policy auto tasks/doc.md"
        );
        assert!(!command.contains("--resume"));
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
}
