use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessInvocationKind {
    Session,
    Claim,
    Compact,
    CompactExchange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedHarnessInvocation {
    pub kind: HarnessInvocationKind,
    pub file: PathBuf,
    pub body: String,
}

pub fn prompt_body_from_text(prompt: &str, file: &Path) -> Option<String> {
    let trimmed = prompt.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(invocation) = parse_agent_doc_invocation(prompt, file.parent().unwrap_or(file)) {
        if invocation.kind == HarnessInvocationKind::Session
            && same_file(&invocation.file, file)
            && !invocation.body.is_empty()
        {
            return Some(invocation.body);
        }
        return None;
    }

    Some(trimmed.to_string())
}

pub fn synthetic_diff_from_body(body: &str) -> String {
    agent_doc_diff::synthetic_added_lines_diff(body, "harness")
}

pub fn parse_agent_doc_invocation(prompt: &str, cwd: &Path) -> Option<ParsedHarnessInvocation> {
    let mut lines = prompt.lines().enumerate();
    let (first_idx, first_line) = lines.find(|(_, line)| !line.trim().is_empty())?;
    let first_trimmed = first_line.trim();
    let tokens = first_trimmed.split_whitespace().collect::<Vec<_>>();

    let (kind, file_token, consumed_tokens) = match tokens.as_slice() {
        ["agent-doc", "claim", file, ..] | ["/agent-doc", "claim", file, ..] => {
            (HarnessInvocationKind::Claim, *file, 3usize)
        }
        ["agent-doc", "compact", "exchange", file, ..]
        | ["/agent-doc", "compact", "exchange", file, ..] => {
            (HarnessInvocationKind::CompactExchange, *file, 4usize)
        }
        ["agent-doc", "compact", file, ..] | ["/agent-doc", "compact", file, ..] => {
            (HarnessInvocationKind::Compact, *file, 3usize)
        }
        ["agent-doc", file, ..] | ["/agent-doc", file, ..] => {
            (HarnessInvocationKind::Session, *file, 2usize)
        }
        _ => return None,
    };

    let first_body = tokens
        .iter()
        .skip(consumed_tokens)
        .copied()
        .collect::<Vec<_>>()
        .join(" ");
    let remaining = prompt
        .lines()
        .enumerate()
        .filter(|(idx, _)| *idx > first_idx)
        .map(|(_, line)| line)
        .collect::<Vec<_>>()
        .join("\n");
    let body = match (first_body.trim(), remaining.trim()) {
        ("", "") => String::new(),
        ("", rest) => rest.to_string(),
        (head, "") => head.to_string(),
        (head, rest) => format!("{head}\n{rest}"),
    };

    let path = PathBuf::from(file_token);
    let resolved = if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    };

    Some(ParsedHarnessInvocation {
        kind,
        file: resolved.canonicalize().unwrap_or(resolved),
        body: body.trim().to_string(),
    })
}

fn same_file(lhs: &Path, rhs: &Path) -> bool {
    let left = lhs.canonicalize().unwrap_or_else(|_| lhs.to_path_buf());
    let right = rhs.canonicalize().unwrap_or_else(|_| rhs.to_path_buf());
    left == right
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_doc() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("task.md");
        fs::write(&doc, "---\n---\n").unwrap();
        (dir, doc)
    }

    #[test]
    fn prompt_body_strips_bare_invocation() {
        let (_dir, doc) = setup_doc();
        let prompt = format!("agent-doc {}", doc.display());

        assert!(prompt_body_from_text(&prompt, &doc).is_none());
    }

    #[test]
    fn prompt_body_extracts_trailing_session_body() {
        let (_dir, doc) = setup_doc();
        let prompt = format!("agent-doc {} #agent-doc-bug", doc.display());

        assert_eq!(
            prompt_body_from_text(&prompt, &doc),
            Some("#agent-doc-bug".to_string())
        );
    }

    #[test]
    fn prompt_body_extracts_following_lines() {
        let (_dir, doc) = setup_doc();
        let prompt = format!(
            "agent-doc {}\ndo #abcd. spec-test-build-install-commit-push",
            doc.display()
        );

        assert_eq!(
            prompt_body_from_text(&prompt, &doc),
            Some("do #abcd. spec-test-build-install-commit-push".to_string())
        );
    }

    #[test]
    fn synthetic_diff_wraps_prompt_body_as_added_lines() {
        let diff = synthetic_diff_from_body("#agent-doc-bug\ndo #abcd");
        assert!(diff.contains("+++ harness"));
        assert!(diff.contains("+#agent-doc-bug"));
        assert!(diff.contains("+do #abcd"));
    }

    #[test]
    fn non_invocation_prompt_is_used_verbatim() {
        let (_dir, doc) = setup_doc();

        assert_eq!(
            prompt_body_from_text("#agent-doc-bug", &doc),
            Some("#agent-doc-bug".to_string())
        );
    }

    #[test]
    fn unrelated_invocation_prompt_is_ignored() {
        let (_dir, doc) = setup_doc();
        let other = doc.with_file_name("other.md");
        let prompt = format!("agent-doc {} #agent-doc-bug", other.display());

        assert!(prompt_body_from_text(&prompt, &doc).is_none());
    }

    #[test]
    fn parser_classifies_non_session_agent_doc_commands() {
        let (_dir, doc) = setup_doc();
        let cwd = doc.parent().unwrap();

        assert_eq!(
            parse_agent_doc_invocation(&format!("agent-doc claim {}", doc.display()), cwd)
                .unwrap()
                .kind,
            HarnessInvocationKind::Claim
        );
        assert_eq!(
            parse_agent_doc_invocation(&format!("agent-doc compact {}", doc.display()), cwd)
                .unwrap()
                .kind,
            HarnessInvocationKind::Compact
        );
        assert_eq!(
            parse_agent_doc_invocation(
                &format!("agent-doc compact exchange {}", doc.display()),
                cwd
            )
            .unwrap()
            .kind,
            HarnessInvocationKind::CompactExchange
        );
    }
}
