//! RunContext-backed frontmatter adapters.
//!
//! Phase 3 / `#lr-config-3`: adds `*_with_context` variants that accept
//! a [`crate::graph::RunContext`] to reuse cached project config + SSH
//! resolution instead of hitting the filesystem on every call.

#![allow(dead_code)]

use anyhow::Result;
use std::path::Path;

use agent_doc_frontmatter::frontmatter::{
    Frontmatter, contextualize_parse_error, ensure_session_with_ssh_resolver,
    parse_with_ssh_resolver,
};

use crate::graph::RunContext;

/// Parse frontmatter using a cached [`RunContext`] for project config and
/// SSH resolution. Avoids redundant filesystem lookups when the caller
/// already has a context from a prior phase (preflight, plan, etc.).
pub fn parse_for_file_with_context<'a>(
    content: &'a str,
    file: &Path,
    rc: &RunContext,
) -> Result<(Frontmatter, &'a str)> {
    let display = file.display().to_string();
    let ssh = rc.ssh_context();
    let ctx = ssh.as_resolver_context(&display);
    parse_with_ssh_resolver(content, &ctx).map_err(|err| contextualize_parse_error(&display, err))
}

/// Ensure a document has a session id using a cached [`RunContext`].
/// Avoids redundant filesystem lookups for project config and SSH resolution.
pub fn ensure_session_for_file_with_context(
    content: &str,
    file: &Path,
    rc: &RunContext,
) -> Result<(String, String)> {
    let display = file.display().to_string();
    let ssh = rc.ssh_context();
    let ctx = ssh.as_resolver_context(&display);
    ensure_session_with_ssh_resolver(content, &ctx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_project(dir: &std::path::Path) -> std::path::PathBuf {
        std::fs::create_dir_all(dir.join(".agent-doc")).unwrap();
        dir.join(".agent-doc").join("config.toml")
    }

    #[test]
    fn parse_for_file_matches_with_context() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("doc.md");
        let content = "---\nagent: opencode\n---\nbody\n";
        std::fs::write(&doc, content).unwrap();

        let rc = RunContext::new(doc.clone());
        let result_direct = agent_doc_frontmatter_io::session::parse_for_file(content, &doc)
            .expect("file-backed parse succeeds");
        let result_ctx = parse_for_file_with_context(content, &doc, &rc).unwrap();

        assert_eq!(result_direct.0.agent, result_ctx.0.agent);
        assert_eq!(result_direct.1, result_ctx.1);
    }

    #[test]
    fn ensure_session_for_file_matches_with_context() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("doc.md");
        let content = "---\nagent: opencode\n---\nbody\n";
        std::fs::write(&doc, content).unwrap();

        let rc = RunContext::new(doc.clone());
        let result_direct =
            agent_doc_frontmatter_io::session::ensure_session_for_file(content, &doc)
                .expect("file-backed ensure succeeds");
        let result_ctx = ensure_session_for_file_with_context(content, &doc, &rc).unwrap();

        assert!(
            result_direct.0.contains("agent_doc_session:"),
            "direct result should have session"
        );
        assert!(
            result_ctx.0.contains("agent_doc_session:"),
            "context result should have session"
        );
        assert!(result_direct.0.contains("agent: opencode"));
        assert!(result_ctx.0.contains("agent: opencode"));
    }
}
