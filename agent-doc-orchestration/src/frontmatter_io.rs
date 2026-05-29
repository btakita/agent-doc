//! Frontmatter I/O — `&Path`-taking wrappers around the pure
//! [`agent_doc_core::frontmatter`] surface. Lives in the main crate so
//! `agent-doc-core` can satisfy plan acceptance criterion #3 ("no
//! `&Path` or `std::fs` in core"). Wave 5 / `#0c4e` of `#adcr`.
//!
//! Re-exported from [`crate::frontmatter`] so existing call sites that
//! use `frontmatter::parse_for_file(content, &Path)`,
//! `frontmatter::ensure_session_for_file(content, &Path)`, and
//! `frontmatter::read_session_id(&Path)` continue resolving unchanged.

#![allow(dead_code)]

use anyhow::Result;
use std::path::Path;

use agent_doc_core::frontmatter::{
    Frontmatter, SshResolverContext, contextualize_parse_error,
    ensure_session_with_ssh_resolver, parse, parse_with_ssh_resolver,
};

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
