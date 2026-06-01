//! Frontmatter I/O — `&Path`-taking wrappers around the pure
//! [`agent_doc_core::frontmatter`] surface. Lives in the main crate so
//! `agent-doc-core` can satisfy plan acceptance criterion #3 ("no
//! `&Path` or `std::fs` in core"). Wave 5 / `#0c4e` of `#adcr`.
//!
//! Re-exported from [`crate::frontmatter`] so existing call sites that
//! use `frontmatter::parse_for_file(content, &Path)`,
//! `frontmatter::ensure_session_for_file(content, &Path)`, and
//! `frontmatter::read_session_id(&Path)` continue resolving unchanged.
//!
//! Phase 3 / `#lr-config-3`: adds `*_with_context` variants that accept
//! a [`crate::graph::RunContext`] to reuse cached project config + SSH
//! resolution instead of hitting the filesystem on every call.

#![allow(dead_code)]

use anyhow::Result;
use std::path::Path;

use agent_doc_core::frontmatter::{
    Frontmatter, SshResolverContext, contextualize_parse_error,
    ensure_session_with_ssh_resolver, parse, parse_with_ssh_resolver,
};

use crate::graph::RunContext;
use crate::project_config;

/// Parse frontmatter for a concrete document path so callers can surface
/// actionable errors. Wraps the pure
/// [`agent_doc_core::frontmatter::parse_with_ssh_resolver`] after resolving
/// the project config + canonical relative path from the filesystem.
pub fn parse_for_file<'a>(content: &'a str, file: &Path) -> Result<(Frontmatter, &'a str)> {
    let display = file.display().to_string();
    let (project, doc_relative) = resolve_ssh_context_inputs(file);
    let ctx = SshResolverContext {
        project: &project,
        doc_relative: &doc_relative,
        file_display: &display,
    };
    parse_with_ssh_resolver(content, &ctx).map_err(|err| contextualize_parse_error(&display, err))
}

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

/// Ensure a document has a session id while preserving the target path in parse errors.
pub fn ensure_session_for_file(content: &str, file: &Path) -> Result<(String, String)> {
    let display = file.display().to_string();
    let (project, doc_relative) = resolve_ssh_context_inputs(file);
    let ctx = SshResolverContext {
        project: &project,
        doc_relative: &doc_relative,
        file_display: &display,
    };
    ensure_session_with_ssh_resolver(content, &ctx)
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

/// Read the session UUID from a document file. Returns `None` if not found.
pub fn read_session_id(file: &Path) -> Option<String> {
    let content = std::fs::read_to_string(file).ok()?;
    let (fm, _) = parse(&content).ok()?;
    fm.session
}

/// Resolve the project config + canonical project-relative path for a
/// document. Both inputs are used to build the [`SshResolverContext`] the
/// pure parsers consume.
fn resolve_ssh_context_inputs(file: &Path) -> (project_config::ProjectConfig, String) {
    let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let project = project_config::load_project_for_doc(&canonical);
    let doc_relative = project_config::project_root_for_doc(&canonical)
        .map(|root| {
            canonical
                .strip_prefix(&root)
                .unwrap_or(canonical.as_path())
                .to_string_lossy()
                .replace('\\', "/")
        })
        .unwrap_or_default();
    (project, doc_relative)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn setup_project(dir: &std::path::Path) -> PathBuf {
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
        let result_direct = parse_for_file(content, &doc).unwrap();
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
        let result_direct = ensure_session_for_file(content, &doc).unwrap();
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
