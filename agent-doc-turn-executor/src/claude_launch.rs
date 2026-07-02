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

    if let Some(sid) = session_id {
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
    args.push(
        "You are responding inside an interactive session document. \
The user edits the document and submits diffs to you. \
Respond concisely in markdown. Classify prompt-bearing inline edits \
as prompt targets vs content edits, and address new ## User blocks \
as well as prompt-bearing changes inside prior responses."
            .to_string(),
    );
    args
}

#[cfg(test)]
mod tests {
    use super::*;

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
