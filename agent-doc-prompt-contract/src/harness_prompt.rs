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

pub fn agent_doc_invocation_file_from_text(prompt: &str) -> Option<&str> {
    let mut inside_code_fence = false;
    for raw_line in prompt.lines().rev() {
        let line = raw_line.trim();
        if line.starts_with("```") {
            inside_code_fence = !inside_code_fence;
            continue;
        }
        if inside_code_fence || line.is_empty() {
            continue;
        }
        let Some(parsed_line) = parse_agent_doc_invocation_line(line) else {
            continue;
        };
        let file = parsed_line.file;
        if file.starts_with('<') && file.ends_with('>') {
            continue;
        }
        return Some(file);
    }
    None
}

pub fn parse_agent_doc_invocation(prompt: &str, cwd: &Path) -> Option<ParsedHarnessInvocation> {
    let mut lines = prompt.lines().enumerate();
    let (first_idx, first_line) = lines.find(|(_, line)| !line.trim().is_empty())?;
    let first_trimmed = first_line.trim();
    let tokens = first_trimmed.split_whitespace().collect::<Vec<_>>();

    let parsed_line = parse_agent_doc_invocation_tokens(&tokens)?;

    let first_body = tokens
        .iter()
        .skip(parsed_line.consumed_tokens)
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

    let path = PathBuf::from(parsed_line.file);
    let resolved = if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    };

    Some(ParsedHarnessInvocation {
        kind: parsed_line.kind,
        file: resolved.canonicalize().unwrap_or(resolved),
        body: body.trim().to_string(),
    })
}

struct AgentDocInvocationLine<'a> {
    kind: HarnessInvocationKind,
    file: &'a str,
    consumed_tokens: usize,
}

fn parse_agent_doc_invocation_line(line: &str) -> Option<AgentDocInvocationLine<'_>> {
    let tokens = line.split_whitespace().collect::<Vec<_>>();
    parse_agent_doc_invocation_tokens(&tokens)
}

fn parse_agent_doc_invocation_tokens<'a>(tokens: &[&'a str]) -> Option<AgentDocInvocationLine<'a>> {
    let (kind, file, consumed_tokens) = match tokens {
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
    Some(AgentDocInvocationLine {
        kind,
        file,
        consumed_tokens,
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

    #[test]
    fn invocation_file_scan_prefers_real_invocation_after_instruction_preamble() {
        let prompt = "# AGENTS.md instructions\n\
\n\
```\n\
agent-doc <FILE>\n\
agent-doc compact <FILE>\n\
```\n\
\n\
Use the harness-native entrypoint below.\n\
\n\
agent-doc tasks/session.md\n";

        assert_eq!(
            agent_doc_invocation_file_from_text(prompt),
            Some("tasks/session.md")
        );
    }

    #[test]
    fn invocation_file_scan_accepts_same_line_body_and_slash_form() {
        assert_eq!(
            agent_doc_invocation_file_from_text("/agent-doc tasks/session.md #agent-doc-bug"),
            Some("tasks/session.md")
        );
    }

    #[test]
    fn invocation_file_scan_continues_past_trailing_prompt_body() {
        assert_eq!(
            agent_doc_invocation_file_from_text("agent-doc tasks/session.md\ndo #abcd"),
            Some("tasks/session.md")
        );
    }

    #[test]
    fn invocation_file_scan_accepts_non_session_commands() {
        assert_eq!(
            agent_doc_invocation_file_from_text("agent-doc claim tasks/session.md"),
            Some("tasks/session.md")
        );
        assert_eq!(
            agent_doc_invocation_file_from_text("agent-doc compact tasks/session.md"),
            Some("tasks/session.md")
        );
        assert_eq!(
            agent_doc_invocation_file_from_text("agent-doc compact exchange tasks/session.md"),
            Some("tasks/session.md")
        );
    }

    #[test]
    fn invocation_file_scan_rejects_placeholders_and_fenced_examples() {
        assert!(agent_doc_invocation_file_from_text("agent-doc <FILE>").is_none());
        assert!(
            agent_doc_invocation_file_from_text("```\nagent-doc tasks/session.md\n```").is_none()
        );
    }
}
