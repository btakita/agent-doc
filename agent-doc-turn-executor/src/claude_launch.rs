const CLAUDE_SESSION_DOCUMENT_PROMPT: &str = "You are responding inside an interactive session document. \
The user edits the document and submits diffs to you. \
Respond concisely in markdown. Classify prompt-bearing inline edits \
as prompt targets vs content edits, and address new ## User blocks \
as well as prompt-bearing changes inside prior responses.";

pub fn default_base_args() -> Vec<String> {
    vec![
        "-p".to_string(),
        "--output-format".to_string(),
        "json".to_string(),
        "--permission-mode".to_string(),
        "acceptEdits".to_string(),
    ]
}

/// Structural minimum args required for non-interactive JSON communication.
/// Permission settings are intentionally excluded -- callers supply those from
/// frontmatter or config.
pub fn structural_base_args() -> Vec<String> {
    vec![
        "-p".to_string(),
        "--output-format".to_string(),
        "json".to_string(),
    ]
}

pub fn claude_json_args(
    base_args: &[String],
    session_id: Option<&str>,
    fork: bool,
    model: Option<&str>,
) -> Vec<String> {
    let mut args = base_args.to_vec();
    append_claude_session_args(&mut args, session_id, fork, model);
    args
}

pub fn claude_streaming_args(
    base_args: &[String],
    session_id: Option<&str>,
    fork: bool,
    model: Option<&str>,
) -> Vec<String> {
    let mut args = Vec::new();
    let mut iter = base_args.iter().peekable();
    while let Some(arg) = iter.next() {
        if arg == "--output-format" {
            iter.next();
            continue;
        }
        if arg.starts_with("--output-format=") {
            continue;
        }
        args.push(arg.clone());
    }
    args.push("--output-format".to_string());
    args.push("stream-json".to_string());
    args.push("--verbose".to_string());

    append_claude_session_args(&mut args, session_id, fork, model);
    args
}

fn append_claude_session_args(
    args: &mut Vec<String>,
    session_id: Option<&str>,
    fork: bool,
    model: Option<&str>,
) {
    if let Some(sid) = session_id {
        strip_claude_fresh_session_id(args);
        args.push("--resume".to_string());
        args.push(sid.to_string());
    } else if fork {
        args.push("--continue".to_string());
        args.push("--fork-session".to_string());
    }

    if let Some(m) = model {
        args.push("--model".to_string());
        args.push(m.to_string());
    }

    args.push("--append-system-prompt".to_string());
    args.push(CLAUDE_SESSION_DOCUMENT_PROMPT.to_string());
}

fn strip_claude_fresh_session_id(args: &mut Vec<String>) {
    let mut retained = Vec::with_capacity(args.len());
    let mut iter = std::mem::take(args).into_iter();
    while let Some(arg) = iter.next() {
        if arg == "--session-id" {
            iter.next();
            continue;
        }
        if arg.starts_with("--session-id=") {
            continue;
        }
        retained.push(arg);
    }
    *args = retained;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn default_base_args_use_claude_json_accept_edits_policy() {
        assert_eq!(
            default_base_args(),
            strings(&[
                "-p",
                "--output-format",
                "json",
                "--permission-mode",
                "acceptEdits"
            ])
        );
    }

    #[test]
    fn structural_base_args_include_prompt_json_only() {
        assert_eq!(
            structural_base_args(),
            strings(&["-p", "--output-format", "json"])
        );
    }

    #[test]
    fn streaming_args_replace_output_format_and_preserve_add_dir() {
        let args = claude_streaming_args(
            &[
                "-p".into(),
                "--output-format".into(),
                "json".into(),
                "--permission-mode".into(),
                "acceptEdits".into(),
                "--add-dir".into(),
                "/tmp/gitdir".into(),
            ],
            None,
            false,
            None,
        );
        assert!(
            args.windows(2)
                .any(|w| w == ["--output-format", "stream-json"])
        );
        assert!(args.contains(&"--verbose".to_string()));
        assert!(args.windows(2).any(|w| w == ["--add-dir", "/tmp/gitdir"]));
        assert!(!args.windows(2).any(|w| w == ["--output-format", "json"]));
    }

    #[test]
    fn json_args_preserve_base_output_format_and_add_session_prompt() {
        let args = claude_json_args(
            &[
                "-p".into(),
                "--output-format".into(),
                "json".into(),
                "--permission-mode".into(),
                "acceptEdits".into(),
            ],
            Some("session-123"),
            true,
            Some("opus"),
        );

        assert!(args.windows(2).any(|w| w == ["--output-format", "json"]));
        assert!(args.windows(2).any(|w| w == ["--resume", "session-123"]));
        assert!(!args.contains(&"--continue".to_string()));
        assert!(!args.contains(&"--fork-session".to_string()));
        assert!(args.windows(2).any(|w| w == ["--model", "opus"]));
        assert!(
            args.windows(2).any(|w| w[0] == "--append-system-prompt"
                && w[1].contains("interactive session document"))
        );
    }

    #[test]
    fn exact_resume_replaces_fresh_session_id_assignment() {
        let args = claude_json_args(
            &[
                "-p".into(),
                "--session-id".into(),
                "fresh-session".into(),
                "--permission-mode".into(),
                "acceptEdits".into(),
            ],
            Some("existing-session"),
            false,
            None,
        );

        assert!(!args.iter().any(|arg| arg == "--session-id"));
        assert!(!args.iter().any(|arg| arg.starts_with("--session-id=")));
        assert!(
            args.windows(2)
                .any(|window| window == ["--resume", "existing-session"])
        );
    }

    #[test]
    fn json_args_fork_when_no_session_id() {
        let args = claude_json_args(&["-p".into()], None, true, None);

        assert!(
            args.windows(2)
                .any(|w| w == ["--continue", "--fork-session"])
        );
        assert!(!args.contains(&"--resume".to_string()));
    }

    #[test]
    fn streaming_args_includes_verbose() {
        let args = claude_streaming_args(
            &["-p".into(), "--output-format".into(), "json".into()],
            None,
            false,
            None,
        );
        assert!(
            args.windows(2)
                .any(|w| w == ["--output-format", "stream-json"]),
            "expected --output-format stream-json in args: {args:?}"
        );
        assert!(
            args.contains(&"--verbose".to_string()),
            "expected --verbose in args (required by Claude CLI when -p + stream-json): {args:?}"
        );
    }
}
