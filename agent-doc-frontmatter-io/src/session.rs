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
    Frontmatter, FrontmatterDefaults, PromptPresets, SshResolverContext,
    apply_required_ssh_defaults, contextualize_parse_error, ensure_session_with_ssh_resolver,
    parse_with_ssh_resolver, session_id_from_content,
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
    /// Project frontmatter defaults captured for the same actor/cycle context.
    /// Long-lived callers replace this context after a project configuration
    /// change rather than performing hidden filesystem reads in pure slots.
    pub frontmatter_defaults: Arc<FrontmatterDefaults>,
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
    // Preserve the established path/line/excerpt diagnostic for malformed
    // document-local YAML before overlaying project defaults.
    parse_with_ssh_resolver(content, &ctx)?;
    let defaults = project_frontmatter_defaults_for_file(file)?;
    let (mut fm, body) = defaults
        .parse(content)
        .map_err(|err| contextualize_parse_error(&display, err))?;
    apply_required_ssh_defaults(&mut fm, &ctx)
        .map_err(|err| contextualize_parse_error(&display, err))?;
    Ok((fm, body))
}

/// Parse frontmatter using a cached SSH/project context.
pub fn parse_for_file_with_context<'a>(
    content: &'a str,
    file: &Path,
    ssh: &ResolvedSshContext,
) -> Result<(Frontmatter, &'a str)> {
    let display = file.display().to_string();
    let ctx = ssh.as_resolver_context(&display);
    parse_with_ssh_resolver(content, &ctx)?;
    let (mut fm, body) = ssh
        .frontmatter_defaults
        .parse(content)
        .map_err(|err| contextualize_parse_error(&display, err))?;
    apply_required_ssh_defaults(&mut fm, &ctx)
        .map_err(|err| contextualize_parse_error(&display, err))?;
    Ok((fm, body))
}

/// Parse the effective frontmatter for a concrete document without applying
/// `.agent-doc/config.toml` SSH defaults. Project frontmatter is read on every
/// call so long-lived callers can invalidate/re-read it alongside config.toml.
fn parse_effective_project_frontmatter<'a>(
    content: &'a str,
    file: &Path,
) -> Result<(Frontmatter, &'a str)> {
    let defaults = project_frontmatter_defaults_for_file(file)?;
    defaults
        .parse(content)
        .map_err(|err| contextualize_parse_error(&file.display().to_string(), err))
}

/// Load the project-frontmatter layers visible to `file`. Farther ancestors
/// are applied first so nearer roots can override them.
pub fn project_frontmatter_defaults_for_file(file: &Path) -> Result<FrontmatterDefaults> {
    let mut defaults = FrontmatterDefaults::default();
    if let Some(nearest_root) = agent_doc_project_config_io::project_root_for_doc(file) {
        let roots = agent_doc_fs::ancestors_within_root_ceiling(&nearest_root);
        for root in roots.into_iter().rev() {
            let project_file = root.join(".agent-doc").join("project.md");
            let project_content = match std::fs::read_to_string(&project_file) {
                Ok(content) => content,
                Err(err) if err.kind() == ErrorKind::NotFound => continue,
                Err(err) => {
                    anyhow::bail!(
                        "failed to read project frontmatter file {}: {err}",
                        project_file.display()
                    );
                }
            };
            defaults.overlay_document(&project_content).map_err(|err| {
                contextualize_parse_error(&project_file.display().to_string(), err)
            })?;
        }
    }
    Ok(defaults)
}

/// Resolve the prompt presets visible to a concrete project document.
///
/// Every agent-doc project root from the nearest root through its ancestors
/// contributes defaults from `.agent-doc/project.md`. Nearer project declarations
/// overlay farther ancestors, and declarations in `content` overlay every project
/// default by name. Project files are read on every call, so edits apply to every
/// existing document without copying or synchronizing frontmatter into those
/// documents. Missing project files are skipped. Other read failures and malformed
/// project frontmatter fail closed with the project-file path in the error.
pub fn effective_prompt_presets_for_file(content: &str, file: &Path) -> Result<PromptPresets> {
    let mut local = parse_effective_project_frontmatter(content, file)?
        .0
        .prompt_presets;
    local.resolve_nested_references().map_err(|err| {
        anyhow::anyhow!(
            "failed to resolve effective prompt presets for {}: {err}",
            file.display()
        )
    })?;
    Ok(local)
}

/// The effective preset map a queue drainability check consults (GH #381).
///
/// Queue drain policy (`agent_doc_queue::queue_continuation::*_with_presets`)
/// and the preflight Todo-backlink audit treat a head whose leading `#name` is
/// an effective preset (document OR inherited from `.agent-doc/project.md`) as
/// a preset invocation, not a reference to tracked work. Those checks run on
/// every idle-watch tick and closeout, so an unreadable or malformed project
/// file falls back to an empty map here: the policy still reads the session
/// document's own `presets`, which is exactly the pre-inheritance behavior.
/// Expansion paths that must fail closed keep calling
/// [`effective_prompt_presets_for_file`] directly.
pub fn effective_prompt_presets_for_drain(content: &str, file: &Path) -> PromptPresets {
    effective_prompt_presets_for_file(content, file).unwrap_or_default()
}

/// Resolve the document-local dogfood cycle switch.
///
/// Dogfood routing is deliberately opt-in: the session document wins when it
/// declares `dogfood`/`agent_doc_dogfood`; otherwise the nearest explicit
/// declaration in `.agent-doc/project.md` is inherited. Absence at every layer
/// is false. This policy gates filing/feedback only; invariant diagnosis and
/// uniquely-safe projection repair remain unconditional.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveDogfood {
    pub enabled: bool,
    pub target: Option<std::path::PathBuf>,
    pub self_target: bool,
    pub auto: bool,
    pub prompt: Option<String>,
}

pub fn effective_dogfood_for_file(content: &str, file: &Path) -> Result<EffectiveDogfood> {
    if let Some(effective) = parse_effective_project_frontmatter(content, file)?
        .0
        .dogfood_mode
    {
        return resolve_dogfood_value(file, &effective);
    }
    Ok(EffectiveDogfood {
        enabled: false,
        target: None,
        self_target: false,
        auto: false,
        prompt: None,
    })
}

fn resolve_dogfood_value(
    file: &Path,
    value: &agent_doc_frontmatter::frontmatter::DogfoodConfig,
) -> Result<EffectiveDogfood> {
    if !value.enabled() {
        return Ok(EffectiveDogfood {
            enabled: false,
            target: None,
            self_target: false,
            auto: false,
            prompt: None,
        });
    }
    let target = match value.target() {
        Some(target) => {
            let explicit_self = agent_doc_project_config_io::project_root_for_doc(file)
                .map(|root| root.join(target))
                .is_some_and(|target| {
                    target.canonicalize().unwrap_or(target)
                        == file.canonicalize().unwrap_or_else(|_| file.to_path_buf())
                });
            if explicit_self {
                Some(file.to_path_buf())
            } else {
                agent_doc_fs::referenced_markdown_path_checked(file, target)?
            }
        }
        None => Some(file.to_path_buf()),
    };
    let canonical_file = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let self_target = target.as_ref().is_some_and(|target| {
        target.canonicalize().unwrap_or_else(|_| target.clone()) == canonical_file
    });
    Ok(EffectiveDogfood {
        enabled: true,
        target,
        self_target,
        auto: value.auto(),
        prompt: value.prompt().map(str::to_owned),
    })
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

/// Return whether the disk replica is already a valid agent-doc document with
/// complete session metadata. This is a read-only bootstrap proof for callers
/// that must not wait for live editor authority when no metadata write is needed.
pub fn session_metadata_ready_on_disk(file: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string(file) else {
        return false;
    };
    if require_agent_doc_document(&content, file).is_err() {
        return false;
    }
    ensure_session_for_file(&content, file)
        .map(|(updated_content, _)| updated_content == content)
        .unwrap_or(false)
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
    fn dogfood_resolution_is_explicit_layered_and_defaults_off() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("tasks/work.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();

        std::fs::write(&doc, "---\nagent_doc_format: template\n---\n").unwrap();
        assert!(
            !effective_dogfood_for_file(&std::fs::read_to_string(&doc).unwrap(), &doc)
                .unwrap()
                .enabled
        );

        std::fs::write(
            dir.path().join(".agent-doc/project.md"),
            "---\ndogfood:\n  path: tasks/project-bugs.md\n  prompt: File a GitHub issue from the project runner.\n---\n",
        )
        .unwrap();
        let inherited =
            effective_dogfood_for_file(&std::fs::read_to_string(&doc).unwrap(), &doc).unwrap();
        assert!(inherited.enabled);
        assert!(
            inherited
                .target
                .as_ref()
                .unwrap()
                .ends_with("tasks/project-bugs.md")
        );
        assert_eq!(
            inherited.prompt.as_deref(),
            Some("File a GitHub issue from the project runner.")
        );

        let local_false = "---\nagent_doc_format: template\ndogfood: false\n---\n";
        assert!(
            !effective_dogfood_for_file(local_false, &doc)
                .unwrap()
                .enabled
        );

        let local_target = "---\nagent_doc_format: template\ndogfood: tasks/local-bugs.md\n---\n";
        let resolved = effective_dogfood_for_file(local_target, &doc).unwrap();
        assert!(resolved.enabled);
        assert!(!resolved.auto);
        assert!(resolved.target.unwrap().ends_with("tasks/local-bugs.md"));
        assert_eq!(
            resolved.prompt, None,
            "a local scalar policy replaces the project map rather than merging its prompt"
        );

        let local_auto = "---\ndogfood:\n  path: tasks/auto-bugs.md\n  auto: true\n  prompt: File a GitHub issue instead of fixing the defect.\n---\n";
        let resolved = effective_dogfood_for_file(local_auto, &doc).unwrap();
        assert!(resolved.enabled);
        assert!(resolved.auto);
        assert!(
            resolved
                .target
                .as_ref()
                .unwrap()
                .ends_with("tasks/auto-bugs.md")
        );
        assert_eq!(
            resolved.prompt.as_deref(),
            Some("File a GitHub issue instead of fixing the defect.")
        );
    }

    #[test]
    fn dogfood_self_target_is_typed_without_recursive_routing() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("tasks/bugs.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "---\ndogfood: tasks/bugs.md\n---\n").unwrap();

        let resolved =
            effective_dogfood_for_file(&std::fs::read_to_string(&doc).unwrap(), &doc).unwrap();
        assert!(resolved.enabled);
        assert!(resolved.self_target);
    }

    #[test]
    fn dogfood_scalar_map_and_layer_precedence_truth_table() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("tasks/work.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        let project_file = dir.path().join(".agent-doc/project.md");

        let cases = [
            // project value, session value, enabled, target suffix/current, auto
            (None, None, false, None, false),
            (Some("dogfood: false"), None, false, None, false),
            (Some("dogfood: true"), None, true, Some("tasks/work.md"), false),
            (
                Some("dogfood: tasks/project-bugs.md"),
                None,
                true,
                Some("tasks/project-bugs.md"),
                false,
            ),
            (
                Some("dogfood:\n  path: tasks/project-auto.md\n  auto: true"),
                None,
                true,
                Some("tasks/project-auto.md"),
                true,
            ),
            (
                Some("dogfood:\n  path: tasks/project-auto.md\n  auto: true"),
                Some("dogfood: false"),
                false,
                None,
                false,
            ),
            (
                Some("dogfood:\n  path: tasks/project-auto.md\n  auto: true"),
                Some("dogfood: true"),
                true,
                Some("tasks/work.md"),
                false,
            ),
            (
                Some("dogfood:\n  path: tasks/project-auto.md\n  auto: true"),
                Some("dogfood:\n  auto: false"),
                true,
                Some("tasks/work.md"),
                false,
            ),
            (
                Some("dogfood: false"),
                Some("dogfood:\n  path: tasks/session-auto.md\n  auto: true"),
                true,
                Some("tasks/session-auto.md"),
                true,
            ),
        ];

        for (project, session, enabled, target_suffix, auto) in cases {
            match project {
                Some(value) => std::fs::write(&project_file, format!("---\n{value}\n---\n"))
                    .unwrap(),
                None => {
                    if project_file.exists() {
                        std::fs::remove_file(&project_file).unwrap();
                    }
                }
            }
            let content = format!("---\n{}\n---\n", session.unwrap_or("agent_doc_format: template"));
            std::fs::write(&doc, &content).unwrap();
            let resolved = effective_dogfood_for_file(&content, &doc).unwrap();
            assert_eq!(resolved.enabled, enabled, "project={project:?} session={session:?}");
            assert_eq!(resolved.auto, auto, "project={project:?} session={session:?}");
            match target_suffix {
                Some(suffix) => assert!(
                    resolved.target.as_ref().is_some_and(|path| path.ends_with(suffix)),
                    "project={project:?} session={session:?} resolved={resolved:?}"
                ),
                None => assert!(resolved.target.is_none(), "resolved={resolved:?}"),
            }
        }
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
    fn project_frontmatter_is_live_for_all_fields_with_document_overrides() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let project_file = dir.path().join(".agent-doc/project.md");
        std::fs::write(
            &project_file,
            "---\nagent: claude\nmodel: project-v1\nagent_doc_clear_threshold: 40\n---\n",
        )
        .unwrap();
        let doc = dir.path().join("tasks/work.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        let content = "---\nmodel: document-model\n---\nbody\n";
        std::fs::write(&doc, content).unwrap();

        let (first, _) = parse_for_file(content, &doc).unwrap();
        assert_eq!(first.agent.as_deref(), Some("claude"));
        assert_eq!(first.model.as_deref(), Some("document-model"));
        assert_eq!(first.clear_threshold, Some(40));

        std::fs::write(
            &project_file,
            "---\nagent: codex\nmodel: project-v2\nagent_doc_clear_threshold: 70\n---\n",
        )
        .unwrap();
        let (second, _) = parse_for_file(content, &doc).unwrap();
        assert_eq!(second.agent.as_deref(), Some("codex"));
        assert_eq!(second.model.as_deref(), Some("document-model"));
        assert_eq!(second.clear_threshold, Some(70));
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), content);
    }

    #[test]
    fn documented_project_wide_preset_example_resolves() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        std::fs::write(
            dir.path().join(".agent-doc/project.md"),
            concat!(
                "---\n",
                "presets:\n",
                "  \"#review\": Review the change and run focused tests.\n",
                "  \"#release\":\n",
                "    prompt: Prepare and publish the release.\n",
                "    runbook: runbooks/release.md\n",
                "---\n\n",
                "# Agent Doc project defaults\n",
            ),
        )
        .unwrap();
        let doc = dir.path().join("tasks/work.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "# Work\n").unwrap();

        let effective = effective_prompt_presets_for_file("# Work\n", &doc).unwrap();
        assert_eq!(
            effective.get("#review").map(String::as_str),
            Some("Review the change and run focused tests.")
        );
        assert_eq!(
            effective.get("#release").map(String::as_str),
            Some("Prepare and publish the release.")
        );
        assert_eq!(effective.runbook("#release"), Some("runbooks/release.md"));
    }

    #[test]
    fn project_preset_references_use_document_overrides() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        std::fs::write(
            dir.path().join(".agent-doc/project.md"),
            "---\npresets:\n  '#generic-preset': 'Check JIRA tickets for #project on {{#board}}'\n  '#project': DEFAULT\n  '#board': General\n---\n",
        )
        .unwrap();
        let doc = dir.path().join("work.md");
        let content = "---\npresets:\n  '#project': AGENT-DOC\n  '#board': Platform\n---\nbody\n";
        std::fs::write(&doc, content).unwrap();

        let effective = effective_prompt_presets_for_file(content, &doc).unwrap();

        assert_eq!(
            effective.get("#generic-preset").map(String::as_str),
            Some("Check JIRA tickets for AGENT-DOC on Platform")
        );
        assert_eq!(
            effective.get("#project").map(String::as_str),
            Some("AGENT-DOC")
        );
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), content);
    }

    #[test]
    fn invalid_effective_project_preset_graph_fails_closed() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let project_file = dir.path().join(".agent-doc/project.md");
        std::fs::write(
            &project_file,
            "---\npresets:\n  '#generic': '#project'\n  '#project': default\n---\n",
        )
        .unwrap();
        let doc = dir.path().join("work.md");
        let content = "---\npresets:\n  '#project':\n---\nbody\n";
        std::fs::write(&doc, content).unwrap();

        let err = effective_prompt_presets_for_file(content, &doc).unwrap_err();

        assert!(
            err.to_string().contains(&doc.display().to_string()),
            "{err:#}"
        );
        assert!(
            err.to_string()
                .contains("nested prompt preset reference #project has no value"),
            "{err:#}"
        );
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
    fn nested_project_presets_overlay_ancestor_presets() {
        let outer = TempDir::new().unwrap();
        setup_project(outer.path());
        let outer_project = outer.path().join(".agent-doc/project.md");
        std::fs::write(
            &outer_project,
            "---\npresets:\n  '#outer': outer\n  '#shared': outer\n  '#masked': outer\n---\n",
        )
        .unwrap();
        let inner = outer.path().join("nested");
        setup_project(&inner);
        std::fs::write(
            inner.join(".agent-doc/project.md"),
            "---\npresets:\n  '#inner': inner\n  '#shared': inner\n  '#masked':\n---\n",
        )
        .unwrap();
        let doc = inner.join("work.md");
        let content = "---\npresets:\n  '#shared': document\n---\nbody\n";
        std::fs::write(&doc, content).unwrap();

        let effective = effective_prompt_presets_for_file(content, &doc).unwrap();
        assert_eq!(effective.get("#inner").map(String::as_str), Some("inner"));
        assert_eq!(effective.get("#outer").map(String::as_str), Some("outer"));
        assert_eq!(
            effective.get("#shared").map(String::as_str),
            Some("document")
        );
        assert!(effective.get("#masked").is_none());
        assert_eq!(effective.unset_keys().collect::<Vec<_>>(), vec!["#masked"]);

        std::fs::write(&outer_project, "---\npresets: [broken\n---\n").unwrap();
        let err = effective_prompt_presets_for_file(content, &doc).unwrap_err();
        assert!(
            err.to_string()
                .contains(&outer_project.display().to_string())
        );
        assert!(err.to_string().contains("invalid YAML frontmatter"));
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
    fn disk_session_metadata_readiness_requires_an_initialized_agent_doc() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let ready = dir.path().join("ready.md");
        std::fs::write(
            &ready,
            "---\nagent_doc_session: tmux-start-ready\n---\nbody\n",
        )
        .unwrap();
        let missing = dir.path().join("missing.md");
        std::fs::write(&missing, "---\nagent: codex\n---\nbody\n").unwrap();
        let plain = dir.path().join("plain.md");
        std::fs::write(&plain, "# Plain notes\n").unwrap();

        assert!(session_metadata_ready_on_disk(&ready));
        assert!(!session_metadata_ready_on_disk(&missing));
        assert!(!session_metadata_ready_on_disk(&plain));
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
            frontmatter_defaults: Arc::default(),
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
            frontmatter_defaults: Arc::default(),
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
