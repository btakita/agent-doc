//! # Module: harness_prompt
//!
//! ## Spec
//! - Resolves harness-side prompt text for a session document when the live file
//!   itself has no prompt-bearing diff.
//! - Supports two binary-owned sources:
//!   1. `AGENT_DOC_HARNESS_PROMPT` env override for non-Codex harnesses/tests.
//!   2. Codex hook session state keyed by the active Codex thread/session id.
//! - Strips the leading `agent-doc <file>` invocation and returns only the
//!   actionable prompt body. Bare invocation with no trailing body yields `None`.
//! - Can synthesize a minimal unified diff so existing diff-based planning and
//!   prompt-contract logic can reuse the same classifiers.
//!
//! ## Agentic Contracts
//! - Only prompt bodies for the exact current document are returned.
//! - Stale or malformed hook state fails closed to `None`.
//! - A harness prompt never mutates the document directly; callers decide whether
//!   to open a cycle from the synthesized diff.

use anyhow::Result;
use std::path::{Path, PathBuf};

#[cfg(test)]
pub static TEST_ENV_LOCK: std::sync::LazyLock<std::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(()));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InvocationKind {
    Session,
    Claim,
    Compact,
    CompactExchange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedInvocation {
    kind: InvocationKind,
    file: PathBuf,
    body: String,
}

pub fn synthetic_diff_for_file(file: &Path) -> Result<Option<String>> {
    let Some(body) = prompt_body_for_file(file)? else {
        return Ok(None);
    };
    Ok(Some(synthetic_diff_from_body(&body)))
}

pub fn prompt_body_for_file(file: &Path) -> Result<Option<String>> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());

    if let Ok(env_prompt) = std::env::var("AGENT_DOC_HARNESS_PROMPT")
        && let Some(body) = prompt_body_from_text(&env_prompt, &canonical)
    {
        return Ok(Some(body));
    }

    if let Some(prompt) = crate::codex_hook::load_prompt_for_current_session(&canonical)?
        && let Some(body) = prompt_body_from_text(&prompt, &canonical)
    {
        return Ok(Some(body));
    }

    Ok(None)
}

fn prompt_body_from_text(prompt: &str, file: &Path) -> Option<String> {
    let trimmed = prompt.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(invocation) = parse_agent_doc_invocation(prompt, file.parent().unwrap_or(file)) {
        if invocation.kind == InvocationKind::Session
            && same_file(&invocation.file, file)
            && !invocation.body.is_empty()
        {
            return Some(invocation.body);
        }
        return None;
    }

    Some(trimmed.to_string())
}

fn same_file(lhs: &Path, rhs: &Path) -> bool {
    let left = lhs.canonicalize().unwrap_or_else(|_| lhs.to_path_buf());
    let right = rhs.canonicalize().unwrap_or_else(|_| rhs.to_path_buf());
    left == right
}

fn synthetic_diff_from_body(body: &str) -> String {
    crate::diff::synthetic_added_lines_diff(body, "harness")
}

fn parse_agent_doc_invocation(prompt: &str, cwd: &Path) -> Option<ParsedInvocation> {
    let mut lines = prompt.lines().enumerate();
    let (first_idx, first_line) = lines.find(|(_, line)| !line.trim().is_empty())?;
    let first_trimmed = first_line.trim();
    let tokens = first_trimmed.split_whitespace().collect::<Vec<_>>();

    let (kind, file_token, consumed_tokens) = match tokens.as_slice() {
        ["agent-doc", "claim", file, ..] | ["/agent-doc", "claim", file, ..] => {
            (InvocationKind::Claim, *file, 3usize)
        }
        ["agent-doc", "compact", "exchange", file, ..]
        | ["/agent-doc", "compact", "exchange", file, ..] => {
            (InvocationKind::CompactExchange, *file, 4usize)
        }
        ["agent-doc", "compact", file, ..] | ["/agent-doc", "compact", file, ..] => {
            (InvocationKind::Compact, *file, 3usize)
        }
        ["agent-doc", file, ..] | ["/agent-doc", file, ..] => {
            (InvocationKind::Session, *file, 2usize)
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

    Some(ParsedInvocation {
        kind,
        file: resolved.canonicalize().unwrap_or(resolved),
        body: body.trim().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    struct EnvGuard {
        key: &'static str,
        prev: Option<String>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let lock = super::TEST_ENV_LOCK.lock().unwrap();
            let prev = std::env::var(key).ok();
            unsafe { std::env::set_var(key, value) };
            Self {
                key,
                prev,
                _lock: lock,
            }
        }

        fn unset(key: &'static str) -> Self {
            let lock = super::TEST_ENV_LOCK.lock().unwrap();
            let prev = std::env::var(key).ok();
            unsafe { std::env::remove_var(key) };
            Self {
                key,
                prev,
                _lock: lock,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(value) = &self.prev {
                unsafe { std::env::set_var(self.key, value) };
            } else {
                unsafe { std::env::remove_var(self.key) };
            }
        }
    }

    fn setup_doc() -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("task.md");
        fs::write(&doc, "---\n---\n").unwrap();
        (dir, doc)
    }

    #[test]
    fn prompt_body_from_env_strips_bare_invocation() {
        let (_dir, doc) = setup_doc();
        let _prompt = EnvGuard::set(
            "AGENT_DOC_HARNESS_PROMPT",
            &format!("agent-doc {}", doc.display()),
        );
        assert!(prompt_body_for_file(&doc).unwrap().is_none());
    }

    #[test]
    fn prompt_body_from_env_extracts_trailing_session_body() {
        let (_dir, doc) = setup_doc();
        let _prompt = EnvGuard::set(
            "AGENT_DOC_HARNESS_PROMPT",
            &format!("agent-doc {} #agent-doc-bug", doc.display()),
        );
        assert_eq!(
            prompt_body_for_file(&doc).unwrap(),
            Some("#agent-doc-bug".to_string())
        );
    }

    #[test]
    fn prompt_body_from_env_extracts_following_lines() {
        let (_dir, doc) = setup_doc();
        let _prompt = EnvGuard::set(
            "AGENT_DOC_HARNESS_PROMPT",
            &format!(
                "agent-doc {}\ndo #abcd. spec-test-build-install-commit-push",
                doc.display()
            ),
        );
        assert_eq!(
            prompt_body_for_file(&doc).unwrap(),
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
    fn non_invocation_env_prompt_is_used_verbatim() {
        let (_dir, doc) = setup_doc();
        let _prompt = EnvGuard::set("AGENT_DOC_HARNESS_PROMPT", "#agent-doc-bug");
        assert_eq!(
            prompt_body_for_file(&doc).unwrap(),
            Some("#agent-doc-bug".to_string())
        );
    }

    #[test]
    fn unrelated_invocation_prompt_is_ignored() {
        let (_dir, doc) = setup_doc();
        let other = doc.with_file_name("other.md");
        let _prompt = EnvGuard::set(
            "AGENT_DOC_HARNESS_PROMPT",
            &format!("agent-doc {} #agent-doc-bug", other.display()),
        );
        assert!(prompt_body_for_file(&doc).unwrap().is_none());
    }

    #[test]
    fn env_guard_unset_restores_absence() {
        let _guard = EnvGuard::unset("AGENT_DOC_HARNESS_PROMPT");
        assert!(std::env::var("AGENT_DOC_HARNESS_PROMPT").is_err());
    }
}
