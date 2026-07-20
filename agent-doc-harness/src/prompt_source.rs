//! # Module: prompt_source
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
use std::path::Path;

pub static TEST_ENV_LOCK: std::sync::LazyLock<parking_lot::Mutex<()>> =
    std::sync::LazyLock::new(|| parking_lot::Mutex::new(()));

pub fn synthetic_diff_for_file(
    file: &Path,
    load_prompt_for_current_session: impl FnOnce(&Path) -> Result<Option<String>>,
) -> Result<Option<String>> {
    let Some(body) = prompt_body_for_file(file, load_prompt_for_current_session)? else {
        return Ok(None);
    };
    Ok(Some(
        agent_doc_prompt_contract::harness_prompt::synthetic_diff_from_body(&body),
    ))
}

pub fn prompt_body_for_file(
    file: &Path,
    load_prompt_for_current_session: impl FnOnce(&Path) -> Result<Option<String>>,
) -> Result<Option<String>> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());

    if let Ok(env_prompt) = std::env::var("AGENT_DOC_HARNESS_PROMPT")
        && let Some(body) = agent_doc_prompt_contract::harness_prompt::prompt_body_from_text(
            &env_prompt,
            &canonical,
        )
    {
        return Ok(Some(body));
    }

    if let Some(prompt) = load_prompt_for_current_session(&canonical)?
        && let Some(body) =
            agent_doc_prompt_contract::harness_prompt::prompt_body_from_text(&prompt, &canonical)
    {
        return Ok(Some(body));
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    struct EnvGuard {
        key: &'static str,
        prev: Option<String>,
        _lock: parking_lot::MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let lock = super::TEST_ENV_LOCK.lock();
            let prev = std::env::var(key).ok();
            unsafe { std::env::set_var(key, value) };
            Self {
                key,
                prev,
                _lock: lock,
            }
        }

        fn unset(key: &'static str) -> Self {
            let lock = super::TEST_ENV_LOCK.lock();
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
        assert!(prompt_body_for_file(&doc, |_| Ok(None)).unwrap().is_none());
    }

    #[test]
    fn prompt_body_from_env_extracts_trailing_session_body() {
        let (_dir, doc) = setup_doc();
        let _prompt = EnvGuard::set(
            "AGENT_DOC_HARNESS_PROMPT",
            &format!("agent-doc {} #agent-doc-bug", doc.display()),
        );
        assert_eq!(
            prompt_body_for_file(&doc, |_| Ok(None)).unwrap(),
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
            prompt_body_for_file(&doc, |_| Ok(None)).unwrap(),
            Some("do #abcd. spec-test-build-install-commit-push".to_string())
        );
    }

    #[test]
    fn non_invocation_env_prompt_is_used_verbatim() {
        let (_dir, doc) = setup_doc();
        let _prompt = EnvGuard::set("AGENT_DOC_HARNESS_PROMPT", "#agent-doc-bug");
        assert_eq!(
            prompt_body_for_file(&doc, |_| Ok(None)).unwrap(),
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
        assert!(prompt_body_for_file(&doc, |_| Ok(None)).unwrap().is_none());
    }

    #[test]
    fn env_guard_unset_restores_absence() {
        let _guard = EnvGuard::unset("AGENT_DOC_HARNESS_PROMPT");
        assert!(std::env::var("AGENT_DOC_HARNESS_PROMPT").is_err());
    }
}
