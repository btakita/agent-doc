//! File/config-backed frontmatter adapters.
//!
//! Pure frontmatter parsing and document opt-in policy live in
//! `agent-doc-frontmatter`. This module owns the filesystem and project-config
//! lookup needed to apply that policy to a concrete document path.

use anyhow::Result;
use std::path::Path;

use agent_doc_frontmatter::frontmatter::{
    Frontmatter, SshResolverContext, contextualize_parse_error, ensure_session_with_ssh_resolver,
    parse_with_ssh_resolver, session_id_from_content,
};
use agent_doc_frontmatter::project_config::{self, ProjectConfig};

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
    session_id_from_content(&content)
}

/// Resolve the project config + canonical project-relative path for a
/// document. Both inputs are used to build the [`SshResolverContext`] the
/// pure parsers consume.
fn resolve_ssh_context_inputs(file: &Path) -> (ProjectConfig, String) {
    let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let project = agent_doc_project_config_io::load_project_for_doc(&canonical);
    let doc_relative = agent_doc_project_config_io::project_root_for_doc(&canonical)
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
/// [`agent_doc_frontmatter::project_config::is_agent_doc_document`] predicate.
pub fn is_agent_doc_document_for_file(content: &str, file: &Path) -> bool {
    let (project, doc_relative) = resolve_ssh_context_inputs(file);
    project_config::is_agent_doc_document(&doc_relative, content, &project)
}

/// Fail closed unless `file` is an opted-in agent-doc document.
///
/// Returns `Ok(())` for documents that carry agent-doc frontmatter, match a
/// `[documents] include` glob, or run under the `auto_session_for_all_md`
/// escape hatch. Otherwise returns an actionable opt-in error and the caller
/// must not mutate the file.
pub fn require_agent_doc_document(content: &str, file: &Path) -> Result<()> {
    let display = file.display();
    let (project, doc_relative) = resolve_ssh_context_inputs(file);
    project_config::require_agent_doc_document(
        &doc_relative,
        content,
        &project,
        &display.to_string(),
    )
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
    fn gate_rejects_plain_md_and_leaves_file_untouched() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("notes.md");
        let content = "# Plain notes\n\nNot a session.\n";
        std::fs::write(&doc, content).unwrap();

        assert!(!is_agent_doc_document_for_file(content, &doc));
        let err = require_agent_doc_document(content, &doc).unwrap_err();
        assert!(err.to_string().contains("is not an agent-doc document"));
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

        let other = dir.path().join("README.md");
        std::fs::write(&other, content).unwrap();
        assert!(!is_agent_doc_document_for_file(content, &other));
    }

    #[test]
    fn ensure_session_for_file_injects_session() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("doc.md");
        let content = "---\nagent: opencode\n---\nbody\n";
        std::fs::write(&doc, content).unwrap();

        let result = ensure_session_for_file(content, &doc).unwrap();

        assert!(result.0.contains("agent_doc_session:"));
        assert!(result.0.contains("agent: opencode"));
    }
}
