//! Pure controller command-line recognition.

use std::path::{Path, PathBuf};

fn arg_file_name_is(arg: &str, expected: &str) -> bool {
    Path::new(arg)
        .file_name()
        .is_some_and(|name| name == expected)
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
}
