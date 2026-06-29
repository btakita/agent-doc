//! Frontmatter I/O — `&Path`-taking wrappers around the pure
//! [`agent_doc_core::frontmatter`] surface. Lives in the main crate so
//! `agent-doc-core` can satisfy plan acceptance criterion #3 ("no
//! `&Path` or `std::fs` in core"). Wave 5 / `#0c4e` of `#adcr`.
//!
//! Re-exported from [`crate::frontmatter`] so existing call sites that
//! use `frontmatter_io::parse_for_file(content, &Path)`,
//! `frontmatter_io::ensure_session_for_file(content, &Path)`, and
//! `frontmatter_io::read_session_id(&Path)` continue resolving unchanged.
//!
//! Phase 3 / `#lr-config-3`: adds `*_with_context` variants that accept
//! a [`crate::graph::RunContext`] to reuse cached project config + SSH
//! resolution instead of hitting the filesystem on every call.

#![allow(dead_code)]

use anyhow::Result;
use std::path::Path;

use agent_doc_frontmatter::frontmatter::{
    Frontmatter, SshResolverContext, contextualize_parse_error, ensure_session_with_ssh_resolver,
    parse, parse_with_ssh_resolver,
};
use agent_doc_frontmatter::project_config::{self, ProjectConfig};

use crate::graph::RunContext;
use crate::project_config_io;

/// Parse frontmatter for a concrete document path so callers can surface
/// actionable errors. Wraps the pure
/// [`agent_doc_frontmatter::frontmatter::parse_with_ssh_resolver`] after resolving
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
fn resolve_ssh_context_inputs(file: &Path) -> (ProjectConfig, String) {
    let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let project = project_config_io::load_project_for_doc(&canonical);
    let doc_relative = project_config_io::project_root_for_doc(&canonical)
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

/// Is `file` an agent-doc session document under the current project config?
///
/// Resolves the project config + project-relative path from the filesystem,
/// then delegates to the pure
/// [`agent_doc_frontmatter::project_config::is_agent_doc_document`] predicate. Used as
/// the opt-in gate so a plain `.md` is not silently converted into a session.
pub fn is_agent_doc_document_for_file(content: &str, file: &Path) -> bool {
    let (project, doc_relative) = resolve_ssh_context_inputs(file);
    project_config::is_agent_doc_document(&doc_relative, content, &project)
}

/// Fail closed unless `file` is an opted-in agent-doc document.
///
/// Returns `Ok(())` for documents that carry agent-doc frontmatter, match a
/// `[documents] include` glob, or run under the `auto_session_for_all_md`
/// escape hatch. Otherwise returns an error with an actionable opt-in message
/// and the caller must **not** mutate the file. Callers run this immediately
/// before [`ensure_session_for_file`] so a plain `.md` is never converted.
pub fn require_agent_doc_document(content: &str, file: &Path) -> Result<()> {
    // A malformed frontmatter block must surface its own contextual parse
    // error downstream (via `ensure_session_for_file`) rather than be masked
    // by the opt-in message — only gate documents that parse cleanly.
    if parse(content).is_err() {
        return Ok(());
    }
    if is_agent_doc_document_for_file(content, file) {
        return Ok(());
    }
    let display = file.display();
    anyhow::bail!(
        "{display} is not an agent-doc document. Run `agent-doc init {display}` to scaffold a session, \
add an `agent_doc_format: template` frontmatter field, or list it under `[documents] include` \
(or set `auto_session_for_all_md = true`) in `.agent-doc/config.toml` to opt in."
    );
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

    #[test]
    fn gate_rejects_plain_md_and_leaves_file_untouched() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("notes.md");
        let content = "# Plain notes\n\nNot a session.\n";
        std::fs::write(&doc, content).unwrap();

        assert!(!is_agent_doc_document_for_file(content, &doc));
        let err = require_agent_doc_document(content, &doc).unwrap_err();
        assert!(err.to_string().contains("is not an agent-doc document"));
        // Gate must not mutate the file (no session injection).
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), content);
    }

    #[test]
    fn gate_allows_frontmatter_opt_in() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("plan.md");
        let content = "---\nagent_doc_format: template\n---\nbody\n";
        std::fs::write(&doc, content).unwrap();

        assert!(is_agent_doc_document_for_file(content, &doc));
        assert!(require_agent_doc_document(content, &doc).is_ok());
    }

    #[test]
    fn gate_allows_config_include_glob() {
        let dir = TempDir::new().unwrap();
        let config_path = setup_project(dir.path());
        std::fs::write(&config_path, "[documents]\ninclude = [\"tasks/**/*.md\"]\n").unwrap();
        std::fs::create_dir_all(dir.path().join("tasks/agent-doc")).unwrap();
        let doc = dir.path().join("tasks/agent-doc/work.md");
        let content = "# plain body, no frontmatter\n";
        std::fs::write(&doc, content).unwrap();

        assert!(is_agent_doc_document_for_file(content, &doc));

        // A sibling outside the glob stays gated.
        let other = dir.path().join("README.md");
        std::fs::write(&other, content).unwrap();
        assert!(!is_agent_doc_document_for_file(content, &other));
    }
}
