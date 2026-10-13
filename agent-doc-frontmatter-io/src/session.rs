//! File/config-backed frontmatter adapters.
//!
//! Pure frontmatter parsing and document opt-in policy live in
//! `agent-doc-frontmatter`. This module owns the filesystem and project-config
//! lookup needed to apply that policy to a concrete document path.

use anyhow::Result;
use std::io::ErrorKind;
use std::path::Path;
use std::sync::Arc;

use agent_doc_frontmatter::frontmatter::{
    Frontmatter, PromptPresets, SshResolverContext, contextualize_parse_error,
    ensure_session_with_ssh_resolver, parse_with_ssh_resolver, session_id_from_content,
};
use agent_doc_frontmatter::project_config::{self, ProjectConfig};

/// Pre-resolved SSH/project context for frontmatter operations.
///
/// Long-lived callers can cache this value once, then parse frontmatter without
/// walking the filesystem for the project config and document-relative path on
/// every call.
#[derive(Debug, Clone)]
pub struct ResolvedSshContext {
    pub config: Arc<ProjectConfig>,
    pub doc_relative: String,
}

impl ResolvedSshContext {
    pub fn as_resolver_context<'a>(&'a self, file_display: &'a str) -> SshResolverContext<'a> {
        SshResolverContext {
            project: &self.config,
            doc_relative: &self.doc_relative,
            file_display,
        }
    }
}

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

/// Parse frontmatter using a cached SSH/project context.
pub fn parse_for_file_with_context<'a>(
    content: &'a str,
    file: &Path,
    ssh: &ResolvedSshContext,
) -> Result<(Frontmatter, &'a str)> {
    let display = file.display().to_string();
    let ctx = ssh.as_resolver_context(&display);
    parse_with_ssh_resolver(content, &ctx).map_err(|err| contextualize_parse_error(&display, err))
}

/// Resolve the prompt presets visible to a concrete project document.
///
/// The nearest agent-doc project root contributes defaults from
/// `.agent-doc/project.md`; declarations in `content` override those defaults by
/// name. The project file is read on every call, so edits apply to every existing
/// document without copying or synchronizing frontmatter into those documents.
/// A missing project file is equivalent to no defaults. Other read failures and
/// malformed project frontmatter fail closed with the project-file path in the
/// error. Root discovery never continues above the nearest project boundary.
pub fn effective_prompt_presets_for_file(content: &str, file: &Path) -> Result<PromptPresets> {
    let display = file.display().to_string();
    let mut local = agent_doc_frontmatter::frontmatter::parse(content)
        .map_err(|err| contextualize_parse_error(&display, err))?
        .0
        .prompt_presets;
    let Some(root) = agent_doc_project_config_io::project_root_for_doc(file) else {
        return Ok(local);
    };
    let project_file = root.join(".agent-doc").join("project.md");
    let project_content = match std::fs::read_to_string(&project_file) {
        Ok(content) => content,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(local),
        Err(err) => {
            anyhow::bail!(
                "failed to read project preset file {}: {err}",
                project_file.display()
            );
        }
    };
    let project_display = project_file.display().to_string();
    let defaults = agent_doc_frontmatter::frontmatter::parse(&project_content)
        .map_err(|err| contextualize_parse_error(&project_display, err))?
        .0
        .prompt_presets;
    local.inherit_defaults(&defaults);
    Ok(local)
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

/// Ensure a document has a session id using a cached SSH/project context.
pub fn ensure_session_for_file_with_context(
    content: &str,
    file: &Path,
    ssh: &ResolvedSshContext,
) -> Result<(String, String)> {
    let display = file.display().to_string();
    let ctx = ssh.as_resolver_context(&display);
    ensure_session_with_ssh_resolver(content, &ctx)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnsureFormattedSessionResult {
    pub assigned: bool,
    pub session_id: Option<String>,
}

/// Assign a session UUID to a formatted agent-doc file that lacks one.
///
/// This is the file-backed initialization adapter for the historical
/// `agent_doc_format` + missing `agent_doc_session` rule. Pure frontmatter owns
/// the predicate; this function owns reading and writing the concrete document.
pub fn ensure_session_uuid_for_formatted_file(file: &Path) -> Result<EnsureFormattedSessionResult> {
    let content = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(_) => {
            return Ok(EnsureFormattedSessionResult {
                assigned: false,
                session_id: None,
            });
        }
    };
    let (fm, _) = match parse_for_file(&content, file) {
        Ok(parsed) => parsed,
        Err(_) => {
            return Ok(EnsureFormattedSessionResult {
                assigned: false,
                session_id: None,
            });
        }
    };
    if !fm.needs_session_for_formatted_document() {
        return Ok(EnsureFormattedSessionResult {
            assigned: false,
            session_id: None,
        });
    }

    let (updated, session_id) = ensure_session_for_file(&content, file)?;
    if updated != content {
        std::fs::write(file, &updated)?;
        Ok(EnsureFormattedSessionResult {
            assigned: true,
            session_id: Some(session_id),
        })
    } else {
        Ok(EnsureFormattedSessionResult {
            assigned: false,
            session_id: Some(session_id),
        })
    }
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
    fn project_presets_are_live_defaults_with_document_overrides() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let project_file = dir.path().join(".agent-doc/project.md");
        std::fs::write(
            &project_file,
            "---\npresets:\n  '#shared': project v1\n  '#inherited':\n    prompt: inherited body\n    runbook: runbooks/inherited.md\n---\n# Project defaults\n",
        )
        .unwrap();
        let doc = dir.path().join("tasks/work.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        let content = "---\npresets:\n  '#shared': local\n  '#local': local body\n---\nbody\n";
        std::fs::write(&doc, content).unwrap();

        let first = effective_prompt_presets_for_file(content, &doc).unwrap();
        assert_eq!(first.get("#shared").map(String::as_str), Some("local"));
        assert_eq!(
            first.get("#inherited").map(String::as_str),
            Some("inherited body")
        );
        assert_eq!(first.runbook("#inherited"), Some("runbooks/inherited.md"));
        assert_eq!(first.get("#local").map(String::as_str), Some("local body"));

        std::fs::write(
            &project_file,
            "---\npresets:\n  '#shared': project v2\n  '#inherited': changed live\n---\n",
        )
        .unwrap();
        let second = effective_prompt_presets_for_file(content, &doc).unwrap();
        assert_eq!(second.get("#shared").map(String::as_str), Some("local"));
        assert_eq!(
            second.get("#inherited").map(String::as_str),
            Some("changed live")
        );
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), content);
    }

    #[test]
    fn local_null_declaration_suppresses_inherited_preset() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        std::fs::write(
            dir.path().join(".agent-doc/project.md"),
            "---\npresets:\n  '#typing': inherited\n---\n",
        )
        .unwrap();
        let doc = dir.path().join("work.md");
        let content = "---\npresets:\n  '#typing':\n---\nbody\n";
        std::fs::write(&doc, content).unwrap();

        let effective = effective_prompt_presets_for_file(content, &doc).unwrap();
        assert!(effective.get("#typing").is_none());
        assert_eq!(effective.unset_keys().collect::<Vec<_>>(), vec!["#typing"]);
    }

    #[test]
    fn missing_project_file_is_empty_and_malformed_file_fails_closed() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("work.md");
        let content = "---\npresets:\n  '#local': body\n---\n";
        std::fs::write(&doc, content).unwrap();

        let local = effective_prompt_presets_for_file(content, &doc).unwrap();
        assert_eq!(local.get("#local").map(String::as_str), Some("body"));

        let project_file = dir.path().join(".agent-doc/project.md");
        std::fs::write(&project_file, "---\npresets: [broken\n---\n").unwrap();
        let err = effective_prompt_presets_for_file(content, &doc).unwrap_err();
        assert!(
            err.to_string()
                .contains(&project_file.display().to_string())
        );
        assert!(err.to_string().contains("invalid YAML frontmatter"));
    }

    #[test]
    fn nearest_project_root_is_the_preset_inheritance_boundary() {
        let outer = TempDir::new().unwrap();
        setup_project(outer.path());
        std::fs::write(
            outer.path().join(".agent-doc/project.md"),
            "---\npresets:\n  '#outer': outer\n---\n",
        )
        .unwrap();
        let inner = outer.path().join("nested");
        setup_project(&inner);
        std::fs::write(
            inner.join(".agent-doc/project.md"),
            "---\npresets:\n  '#inner': inner\n---\n",
        )
        .unwrap();
        let doc = inner.join("work.md");
        std::fs::write(&doc, "body\n").unwrap();

        let effective = effective_prompt_presets_for_file("body\n", &doc).unwrap();
        assert_eq!(effective.get("#inner").map(String::as_str), Some("inner"));
        assert!(effective.get("#outer").is_none());
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

    #[test]
    fn ensure_session_uuid_for_formatted_file_assigns_when_missing() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("doc.md");
        let content = "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\nbody\n";
        std::fs::write(&doc, content).unwrap();

        let result = ensure_session_uuid_for_formatted_file(&doc).unwrap();

        assert!(result.assigned);
        assert!(result.session_id.is_some());
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("agent_doc_session:"));
        assert!(updated.contains("agent_doc_format:"));
    }

    #[test]
    fn ensure_session_uuid_for_formatted_file_noops_when_session_exists_or_unformatted() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let with_session = dir.path().join("with-session.md");
        let with_session_content =
            "---\nagent_doc_session: existing\nagent_doc_format: template\n---\nbody\n";
        std::fs::write(&with_session, with_session_content).unwrap();
        let plain = dir.path().join("plain.md");
        let plain_content = "---\ntitle: notes\n---\nbody\n";
        std::fs::write(&plain, plain_content).unwrap();

        let existing = ensure_session_uuid_for_formatted_file(&with_session).unwrap();
        let unformatted = ensure_session_uuid_for_formatted_file(&plain).unwrap();

        assert!(!existing.assigned);
        assert_eq!(
            std::fs::read_to_string(&with_session).unwrap(),
            with_session_content
        );
        assert!(!unformatted.assigned);
        assert_eq!(std::fs::read_to_string(&plain).unwrap(), plain_content);
    }

    #[test]
    fn context_parse_matches_file_backed_parse() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("doc.md");
        let content = "---\nagent: opencode\n---\nbody\n";
        std::fs::write(&doc, content).unwrap();

        let result_direct = parse_for_file(content, &doc).expect("file-backed parse succeeds");
        let ssh = ResolvedSshContext {
            config: Arc::new(ProjectConfig::default()),
            doc_relative: "doc.md".to_string(),
        };
        let result_ctx = parse_for_file_with_context(content, &doc, &ssh).unwrap();

        assert_eq!(result_direct.0.agent, result_ctx.0.agent);
        assert_eq!(result_direct.1, result_ctx.1);
    }

    #[test]
    fn context_ensure_session_matches_file_backed_ensure() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("doc.md");
        let content = "---\nagent: opencode\n---\nbody\n";
        std::fs::write(&doc, content).unwrap();

        let result_direct =
            ensure_session_for_file(content, &doc).expect("file-backed ensure succeeds");
        let ssh = ResolvedSshContext {
            config: Arc::new(ProjectConfig::default()),
            doc_relative: "doc.md".to_string(),
        };
        let result_ctx = ensure_session_for_file_with_context(content, &doc, &ssh).unwrap();

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
