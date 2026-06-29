//! Pure controller command-line recognition.

use std::path::{Path, PathBuf};

fn arg_file_name_is(arg: &str, expected: &str) -> bool {
    Path::new(arg)
        .file_name()
        .is_some_and(|name| name == expected)
}

fn token_basename(token: &str) -> &str {
    Path::new(token)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(token)
}

fn token_is_agent_doc_binary(token: &str) -> bool {
    token_basename(token).starts_with("agent-doc")
}

fn token_is_harness_binary(token: &str) -> bool {
    matches!(
        token_basename(token),
        "claude" | "codex" | "opencode" | "bun" | "node"
    )
}

fn token_is_non_owner_agent_doc_subcommand(token: &str) -> bool {
    matches!(token, "route" | "claim")
}

fn is_shell_c_controller_sentinel(args: &[String], agent_doc_idx: usize) -> bool {
    agent_doc_idx >= 3
        && args.get(agent_doc_idx - 2).is_some_and(|arg| arg == "-c")
        && args.first().is_some_and(|arg| {
            ["sh", "bash", "dash", "zsh"]
                .iter()
                .any(|shell| arg_file_name_is(arg, shell))
        })
}

pub fn agent_doc_controller_serve_arg_index(args: &[String]) -> Option<usize> {
    args.windows(3).enumerate().find_map(|(idx, window)| {
        (arg_file_name_is(&window[0], "agent-doc")
            && window[1] == "controller"
            && window[2] == "serve"
            && (idx == 0 || is_shell_c_controller_sentinel(args, idx)))
        .then_some(idx)
    })
}

pub fn controller_serve_project_root_from_args(args: &[String]) -> Option<PathBuf> {
    let controller_idx = agent_doc_controller_serve_arg_index(args)?;
    args[controller_idx + 3..]
        .windows(2)
        .find_map(|window| (window[0] == "--project-root").then(|| PathBuf::from(&window[1])))
}

/// True when `cmdline` is a long-lived agent-doc/harness owner invocation for
/// some document, regardless of which document.
pub fn cmdline_is_agent_doc_owner_session(cmdline: &str) -> bool {
    let tokens = cmdline.split_whitespace().collect::<Vec<_>>();
    if let Some(idx) = tokens
        .iter()
        .position(|token| token_is_agent_doc_binary(token))
    {
        let Some(next) = tokens.get(idx + 1) else {
            return false;
        };
        if *next == "start" {
            return true;
        }
        return !token_is_non_owner_agent_doc_subcommand(next);
    }

    tokens.iter().any(|token| token_is_harness_binary(token))
}

/// True when `cmdline` references at least one `.md` document path token.
pub fn cmdline_references_md_document(cmdline: &str) -> bool {
    cmdline.split_whitespace().any(|token| {
        token
            .trim_matches(|c| c == '"' || c == '\'')
            .ends_with(".md")
    })
}

/// First `.md` document path token in `cmdline`, for cross-document diagnostics.
pub fn owner_document_from_cmdline(cmdline: &str) -> Option<String> {
    cmdline
        .split_whitespace()
        .map(|token| token.trim_matches(|c| c == '"' || c == '\''))
        .find(|token| token.ends_with(".md"))
        .map(|token| token.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_serve_project_root_from_args_extracts_direct_and_shell_sentinel() {
        let args = vec![
            "/some/bin/agent-doc".to_string(),
            "controller".to_string(),
            "serve".to_string(),
            "--project-root".to_string(),
            "/home/me/work/sample-app".to_string(),
            "--handoff-state".to_string(),
            "preparing".to_string(),
        ];
        assert_eq!(
            controller_serve_project_root_from_args(&args),
            Some(PathBuf::from("/home/me/work/sample-app"))
        );

        let shell_sentinel = vec![
            "sh".to_string(),
            "-c".to_string(),
            "sleep 30; :".to_string(),
            "/home/me/work/sample-app/agent-doc".to_string(),
            "controller".to_string(),
            "serve".to_string(),
            "--project-root".to_string(),
            "/home/me/work/sample-app".to_string(),
            "--handoff-state".to_string(),
            "preparing".to_string(),
        ];
        assert_eq!(
            controller_serve_project_root_from_args(&shell_sentinel),
            Some(PathBuf::from("/home/me/work/sample-app"))
        );
    }

    #[test]
    fn controller_serve_project_root_from_args_rejects_non_controllers() {
        assert_eq!(
            controller_serve_project_root_from_args(&[
                "/bin/agent-doc".to_string(),
                "controller".to_string(),
                "serve".to_string(),
            ]),
            None
        );
        assert_eq!(
            controller_serve_project_root_from_args(&[
                "/bin/agent-doc".to_string(),
                "status".to_string(),
                "--project-root".to_string(),
                "/x".to_string(),
            ]),
            None
        );
        assert_eq!(
            controller_serve_project_root_from_args(&[
                "sleep".to_string(),
                "controller".to_string(),
                "serve".to_string(),
                "--project-root".to_string(),
                "/x".to_string(),
            ]),
            None
        );
        assert_eq!(
            controller_serve_project_root_from_args(&[
                "tmux".to_string(),
                "new-session".to_string(),
                "agent-doc".to_string(),
                "controller".to_string(),
                "serve".to_string(),
                "--project-root".to_string(),
                "/x".to_string(),
            ]),
            None
        );
    }

    #[test]
    fn cmdline_owner_session_recognizes_supervisors_and_harnesses() {
        assert!(cmdline_is_agent_doc_owner_session(
            "/home/me/.cargo/bin/agent-doc start --route-owned tasks/doc.md"
        ));
        assert!(cmdline_is_agent_doc_owner_session(
            "/usr/bin/codex /work/project/tasks/doc.md"
        ));
        assert!(!cmdline_is_agent_doc_owner_session(
            "/home/me/.cargo/bin/agent-doc route tasks/doc.md"
        ));
        assert!(!cmdline_is_agent_doc_owner_session(
            "/home/me/.cargo/bin/agent-doc claim tasks/doc.md --pane %1"
        ));
        assert!(!cmdline_is_agent_doc_owner_session("-zsh"));
    }

    #[test]
    fn owner_document_from_cmdline_extracts_bound_document() {
        assert_eq!(
            owner_document_from_cmdline(
                "/home/me/.cargo/bin/agent-doc start --route-owned tasks/software/tsift.md"
            ),
            Some("tasks/software/tsift.md".to_string())
        );
        assert_eq!(
            owner_document_from_cmdline("/usr/bin/codex \"tasks/agent-doc/agent-doc-bugs2.md\""),
            Some("tasks/agent-doc/agent-doc-bugs2.md".to_string())
        );
        assert_eq!(owner_document_from_cmdline("-zsh"), None);
    }
}
