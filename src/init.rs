//! # Module: init
//!
//! ## Spec
//! - `run(file, title, agent, mode, artifact, config)`: dispatches on whether a file path was provided.
//!   - No file: calls `init_project()` — checks prerequisites, creates `.agent-doc/`, and installs
//!     `SKILL.md` for the detected harness (generic `.agent/skills/` when none is detected, in
//!     which case it prints the `skill install --harness` command to run), then prints next
//!     steps ending in the README "Getting started" link. Per-document state is stored in the typed ledger, not a filesystem sidecar.
//!     `--artifact` without a session file is an error and does not initialize a project.
//!   - With file: calls `init_file()` — lazily runs project init if `.agent-doc/` is absent, then
//!     scaffolds a new session document.  Fails if the file already exists.
//!   - With `--artifact PLAN.md`: the session is a template control document whose `agent:run`
//!     names only that path. `PLAN.md` is ordinary markdown (no `agent:*` marker, no
//!     `agent_doc_format`, no `## Draft`). `--mode append` is refused.
//! - `init_file` scaffolds two document formats depending on `mode`:
//!   - `"template"` or `"stream"`: frontmatter with `agent_doc_format: template` and
//!     `agent_doc_write: crdt`; body contains a single `<!-- agent:exchange -->` component.
//!   - Any other value (default `"append"`): frontmatter with `agent_doc_session` + `agent` only;
//!     inline `## User` block format.
//! - A UUID v4 is generated for `agent_doc_session` on every scaffolded document.
//! - Parent directories are created with `create_dir_all` if absent.
//! - Agent defaults: `--agent` flag > `config.default_agent` > `"claude"`.
//!
//! ## Agentic Contracts
//! - Returns `Err` if the target file already exists.
//! - Project init is idempotent: re-running when `.agent-doc/` exists is a no-op for directory
//!   creation (via `create_dir_all`).
//! - No git operations are performed during init; the caller is responsible for committing.
//! - `SKILL.md` installation is delegated to `crate::skill::install_and_check_updated`.
//!
//! ## Evals
//! - project_init_next_steps_name_the_harness_install_when_none_was_detected: with no detected
//!   harness, init names `agent-doc skill install --harness <h>` instead of claiming a
//!   `.claude/skills` install; both variants point to the Getting started section.
//! - init_project_creates_dirs: `.agent-doc` is created without a snapshots sidecar directory.
//! - init_file_template_mode: `mode="template"` → scaffolded file contains `agent_doc_format: template` and `agent:exchange` component
//! - init_file_append_mode: `mode="append"` → scaffolded file contains `## User` block, no exchange component
//! - init_file_already_exists: target file exists → returns Err
//! - artifact_session_is_template_aimed_at_one_plain_markdown_file: `--artifact` writes a template
//!   session whose queue names only that file, and the artifact has no agent markers.

use anyhow::Result;
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

use agent_doc_config::Config;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionPreparation {
    Existing,
    Initialized,
}

pub fn title_from_path(file: &Path) -> String {
    let stem = file
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("Untitled Session");
    let title = stem.replace(['-', '_'], " ");
    let mut chars = title.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => "Untitled Session".to_string(),
    }
}

pub fn prepare_editor_session(
    file: &Path,
    title: Option<&str>,
    agent: Option<&str>,
    config: &Config,
) -> Result<SessionPreparation> {
    prepare_editor_session_with(file, title, agent, config, || {
        agent_doc_project_root_io::ensure_project_root_allowed(&std::env::current_dir()?)?;
        if !Path::new(".agent-doc").exists() {
            init_project()?;
        }
        Ok(())
    })
}

/// [`prepare_editor_session`] with the working-directory project init
/// injected. The init acts on the process CWD, which for a test is its crate
/// checkout, so tests supply their own (`#testisolationleaks`).
fn prepare_editor_session_with(
    file: &Path,
    title: Option<&str>,
    agent: Option<&str>,
    config: &Config,
    ensure_project: impl FnOnce() -> Result<()>,
) -> Result<SessionPreparation> {
    let existing = std::fs::read_to_string(file)?;
    if agent_doc_frontmatter::session_id_from_content(&existing).is_some() {
        return Ok(SessionPreparation::Existing);
    }

    ensure_project()?;
    let title = title
        .map(str::to_owned)
        .unwrap_or_else(|| title_from_path(file));
    let agent = agent
        .or(config.default_agent.as_deref())
        .unwrap_or("claude");
    let session_id = Uuid::new_v4();
    let initial = existing.trim_end();
    let initial = if initial.is_empty() {
        String::new()
    } else {
        format!("{initial}\n")
    };
    let components =
        agent_doc_document::claim_scaffold::render_default_template_components(&initial);
    let content = format!(
        "---\nagent_doc_session: {session_id}\nagent: {agent}\nagent_doc_format: template\nagent_doc_write: crdt\nenable_tool_search: true\n---\n\n# {title}{components}"
    );
    std::fs::write(file, content)?;
    eprintln!("Initialized {}", file.display());
    Ok(SessionPreparation::Initialized)
}

/// Initialize a project (no file given): check prereqs, create .agent-doc/, install skill.
fn init_project() -> Result<()> {
    agent_doc_project_root_io::ensure_project_root_allowed(&std::env::current_dir()?)?;
    // Check prerequisites
    crate::install::check_prereqs();

    // Create .agent-doc/ directory structure
    let agent_doc_dir = Path::new(".agent-doc");
    // `#testisolationleaks`: project init writes the CWD project and installs
    // the skill into its root. A test's CWD is its checkout, never a fixture.
    agent_doc_fs::guard_test_runtime_target(agent_doc_dir, "a CWD project init");
    std::fs::create_dir_all(agent_doc_dir)?;
    eprintln!("[init] Created {}", agent_doc_dir.display());

    // Install SKILL.md. The installer prints the real harness and path.
    let generic = crate::skill::install_env_is_generic();
    crate::skill::install_and_check_updated()?;

    for line in project_init_next_steps(generic) {
        eprintln!("{line}");
    }

    Ok(())
}

/// The "what now" lines `agent-doc init` prints after project init.
///
/// `generic` is true when no harness was detected from the calling shell (the
/// usual case for a new user running `init` in a plain terminal), so the skill
/// went to `.agent/skills/`, which no harness reads on its own.
pub(crate) fn project_init_next_steps(generic: bool) -> Vec<String> {
    let mut lines = Vec::new();
    if generic {
        lines.push(
            "[init] No agent harness detected from this shell, so the skill went to .agent/skills/ (no harness reads it automatically).".to_string(),
        );
        lines.push(
            "[init]   Install it for your agent: agent-doc skill install --harness claude   # or: codex, grok, opencode, --all".to_string(),
        );
    }
    lines.push("[init] Project initialized. Next steps:".to_string());
    lines.push(
        "[init]   agent-doc init notes.md \"My Topic\"   # create a session document".to_string(),
    );
    lines.push(
        "[init]   write a request in it, then run a turn: your editor plugin, /agent-doc notes.md in Claude Code, or agent-doc run notes.md".to_string(),
    );
    lines.push("[init]   .agent-doc/ holds local runtime state; add it to .gitignore".to_string());
    lines.push(format!("[init]   Getting started: {GETTING_STARTED_URL}"));
    lines
}

/// Public entry point for a new user, printed by `init` and `--help`.
pub(crate) const GETTING_STARTED_URL: &str = "https://github.com/btakita/agent-doc#getting-started";

/// Scaffold a new session document, lazily initializing the project if needed.
fn init_file(
    file: &Path,
    title: Option<&str>,
    agent: Option<&str>,
    mode: Option<&str>,
    artifact: Option<&Path>,
    config: &Config,
) -> Result<()> {
    agent_doc_project_root_io::ensure_project_root_allowed(&std::env::current_dir()?)?;
    // Lazy project init: if .agent-doc/ doesn't exist, run project init first.
    if !Path::new(".agent-doc").exists() {
        eprintln!("[init] No .agent-doc/ found — running project init first.");
        init_project()?;
    }

    write_new_session(file, title, agent, mode, artifact, config)
}

/// Write a new session, and when `artifact` is set, the plain markdown file it names.
///
/// Callers that already decided project init may call this directly. It does not
/// touch `.agent-doc/` or the process working directory.
fn write_new_session(
    file: &Path,
    title: Option<&str>,
    agent: Option<&str>,
    mode: Option<&str>,
    artifact: Option<&Path>,
    config: &Config,
) -> Result<()> {
    if file.exists() {
        anyhow::bail!("file already exists: {}", file.display());
    }
    if let Some(artifact) = artifact {
        if artifact.exists() {
            anyhow::bail!("file already exists: {}", artifact.display());
        }
        if paths_name_the_same_file(file, artifact) {
            anyhow::bail!(
                "artifact path must be a different file from the session: {}",
                artifact.display()
            );
        }
        require_artifact_template_mode(mode)?;
    }

    let label = artifact.map(artifact_queue_label).transpose()?;
    let title = match (title, artifact) {
        (Some(title), _) => title.to_string(),
        (None, Some(artifact)) => title_from_path(artifact),
        (None, None) => "Untitled Session".to_string(),
    };
    let agent = agent
        .or(config.default_agent.as_deref())
        .unwrap_or("claude");
    let session_id = Uuid::new_v4();
    let mode = mode.unwrap_or(if artifact.is_some() {
        "template"
    } else {
        "append"
    });

    let content = if mode == "template" || mode == "stream" {
        let components = agent_doc_document::claim_scaffold::render_default_template_components("");
        let content = format!(
            "---\nagent_doc_session: {session_id}\nagent: {agent}\nagent_doc_format: template\nagent_doc_write: crdt\nenable_tool_search: true\n---\n\n# {title}{components}"
        );
        if let Some(label) = label.as_deref() {
            aim_queue_at_artifact(&content, label)?
        } else {
            content
        }
    } else {
        format!(
            "---\nagent_doc_session: {session_id}\nagent: {agent}\n---\n\n# Session: {title}\n\n## User\n\n"
        )
    };

    if let Some(artifact) = artifact {
        write_artifact_file(artifact)?;
    }
    if let Err(error) = write_session_file(file, &content) {
        if let Some(artifact) = artifact {
            let _ = std::fs::remove_file(artifact);
        }
        return Err(error);
    }
    if let Some(artifact) = artifact {
        eprintln!("Created artifact {}", artifact.display());
    }
    eprintln!("Created {}", file.display());
    Ok(())
}

fn require_artifact_template_mode(mode: Option<&str>) -> Result<()> {
    match mode {
        None | Some("template") | Some("stream") => Ok(()),
        Some(other) => anyhow::bail!(
            "an artifact session is a template session; --mode {other} cannot be combined with --artifact"
        ),
    }
}

fn artifact_queue_label(artifact: &Path) -> Result<String> {
    let name = artifact
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    if !name.ends_with(".md") || name == ".md" {
        anyhow::bail!(
            "artifact file must be markdown (.md): {}",
            artifact.display()
        );
    }
    let raw = artifact.to_string_lossy().into_owned();
    if raw.contains('\n') || raw.contains('\r') || raw.contains("<!--") || raw.contains("-->") {
        anyhow::bail!(
            "artifact path must be a single line and must not contain a marker: {}",
            artifact.display()
        );
    }
    let label = raw.strip_prefix("./").map(str::to_owned).unwrap_or(raw);
    if label.is_empty() {
        anyhow::bail!("artifact path is empty");
    }
    Ok(label)
}

fn artifact_markdown(artifact: &Path) -> String {
    format!("# {}\n", title_from_path(artifact))
}

fn aim_queue_at_artifact(content: &str, label: &str) -> Result<String> {
    let components = agent_doc_element::element::parse(content)?;
    let queue = components
        .iter()
        .find(|component| component.name == agent_doc_element::element::QUEUE_COMPONENT)
        .ok_or_else(|| anyhow::anyhow!("artifact session scaffold is missing agent:run"))?;
    // The scaffold opener already ends at the following newline, so the row
    // starts on the next line without a blank line above it.
    Ok(queue.replace_content(content, &format!("- [ ] {label}\n")))
}

fn write_artifact_file(artifact: &Path) -> Result<()> {
    if let Some(parent) = artifact.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(artifact, artifact_markdown(artifact))?;
    Ok(())
}

fn write_session_file(file: &Path, content: &str) -> Result<()> {
    if let Some(parent) = file.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(file, content)?;
    Ok(())
}

fn paths_name_the_same_file(left: &Path, right: &Path) -> bool {
    fn normalized(path: &Path) -> PathBuf {
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(path)
        };
        let mut out = PathBuf::new();
        for component in path.components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    if !out.pop() {
                        out.push("..");
                    }
                }
                other => out.push(other.as_os_str()),
            }
        }
        out
    }
    normalized(left) == normalized(right)
}

pub fn run(
    file: Option<&Path>,
    title: Option<&str>,
    agent: Option<&str>,
    mode: Option<&str>,
    artifact: Option<&Path>,
    config: &Config,
) -> Result<()> {
    if artifact.is_some() && file.is_none() {
        anyhow::bail!("--artifact requires a session document path");
    }
    match file {
        None => init_project(),
        Some(path) => init_file(path, title, agent, mode, artifact, config),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_init_next_steps_name_the_harness_install_when_none_was_detected() {
        let generic = project_init_next_steps(true).join("\n");
        assert!(
            generic.contains("agent-doc skill install --harness claude"),
            "{generic}"
        );
        // Init must never claim a `.claude/skills` install it did not make.
        assert!(!generic.contains("Installed .claude/skills"), "{generic}");

        let detected = project_init_next_steps(false).join("\n");
        assert!(
            !detected.contains("No agent harness detected"),
            "{detected}"
        );
        for lines in [&generic, &detected] {
            assert!(lines.contains("agent-doc init notes.md"), "{lines}");
            assert!(lines.contains(".gitignore"), "{lines}");
            assert!(lines.contains(GETTING_STARTED_URL), "{lines}");
        }
    }

    #[test]
    fn editor_session_uses_filename_title_and_preserves_plain_markdown() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("release-notes.md");
        std::fs::write(&file, "Keep this prompt.\n").unwrap();

        let project_init = std::cell::Cell::new(false);
        let outcome =
            prepare_editor_session_with(&file, None, Some("codex"), &Config::default(), || {
                project_init.set(true);
                Ok(())
            })
            .unwrap();
        let content = std::fs::read_to_string(&file).unwrap();

        assert_eq!(outcome, SessionPreparation::Initialized);
        assert!(
            project_init.get(),
            "a new session ensures its project first"
        );
        assert!(content.contains("# Release notes"));
        assert!(content.contains("agent_doc_format: template"));
        let components = agent_doc_element::element::parse(&content).unwrap();
        assert_eq!(
            components
                .iter()
                .map(|component| component.name.as_str())
                .collect::<Vec<_>>(),
            [
                "status",
                "worktrees",
                "recap",
                "active",
                "queue",
                "review",
                "todo",
                "backlog",
                "icebox",
                "notes",
                "done",
                "exchange",
            ]
        );
        let exchange = components.last().unwrap();
        assert_eq!(exchange.content(&content), "Keep this prompt.\n");
    }

    #[test]
    fn editor_session_leaves_existing_session_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("existing.md");
        let content = "---\nagent_doc_session: existing\n---\nBody\n";
        std::fs::write(&file, content).unwrap();

        let outcome = prepare_editor_session_with(&file, None, None, &Config::default(), || {
            panic!("an existing session must not initialize a project")
        })
        .unwrap();

        assert_eq!(outcome, SessionPreparation::Existing);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), content);
    }

    #[test]
    fn artifact_session_is_template_aimed_at_one_plain_markdown_file() {
        let dir = tempfile::tempdir().unwrap();
        let session = dir.path().join("sessions").join("plan-session.md");
        let artifact = dir.path().join("plans").join("my-plan.md");

        write_new_session(
            &session,
            None,
            Some("claude"),
            None,
            Some(&artifact),
            &Config::default(),
        )
        .unwrap();

        let session_text = std::fs::read_to_string(&session).unwrap();
        let artifact_text = std::fs::read_to_string(&artifact).unwrap();
        let label = artifact.display().to_string();
        assert_eq!(
            artifact_queue_label(Path::new("plans/my-plan.md")).unwrap(),
            "plans/my-plan.md"
        );
        assert_eq!(
            artifact_queue_label(Path::new("./plans/my-plan.md")).unwrap(),
            "plans/my-plan.md"
        );

        assert!(session_text.contains("agent_doc_format: template"));
        assert!(session_text.contains("<!-- agent:run"));
        assert!(session_text.contains("<!-- agent:exchange"));
        assert!(!session_text.contains("agent:artifact"));
        assert!(session_text.contains("# My plan"));

        let components = agent_doc_element::element::parse(&session_text).unwrap();
        assert!(
            components
                .iter()
                .any(|component| component.name == "exchange"),
            "session keeps agent:exchange"
        );
        for component in &components {
            let body = component.content(&session_text);
            if component.name == agent_doc_element::element::QUEUE_COMPONENT {
                assert_eq!(body, format!("- [ ] {label}\n"));
                assert_eq!(body.matches("- [ ]").count(), 1);
            } else {
                assert_eq!(
                    body.matches("- [ ]").count(),
                    0,
                    "{} must not name other work",
                    component.name
                );
            }
        }

        assert!(artifact_text.starts_with("# My plan\n"));
        assert!(!artifact_text.contains("<!-- agent:"));
        assert!(!artifact_text.contains("agent_doc_format"));
        assert!(!artifact_text.contains("## Draft"));
        assert!(!artifact_text.contains("agent:run"));
        assert!(!artifact_text.contains("agent:exchange"));
        assert!(!artifact_text.contains("agent:artifact"));
    }

    #[test]
    fn artifact_session_refuses_append_mode_and_a_missing_session_file() {
        let dir = tempfile::tempdir().unwrap();
        let session = dir.path().join("session.md");
        let artifact = dir.path().join("plan.md");
        let error = write_new_session(
            &session,
            None,
            None,
            Some("append"),
            Some(&artifact),
            &Config::default(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("template session"), "{error}");
        assert!(!session.exists());
        assert!(!artifact.exists());

        let error = run(
            None,
            None,
            None,
            None,
            Some(Path::new("plan.md")),
            &Config::default(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("--artifact"), "{error}");
    }

    #[test]
    fn artifact_session_refuses_an_existing_or_non_markdown_artifact() {
        let dir = tempfile::tempdir().unwrap();
        let session = dir.path().join("session.md");
        let artifact = dir.path().join("plan.md");
        std::fs::write(&artifact, "already\n").unwrap();
        let error = write_new_session(
            &session,
            None,
            None,
            Some("template"),
            Some(&artifact),
            &Config::default(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("already exists"), "{error}");
        assert!(!session.exists());
        assert_eq!(std::fs::read_to_string(&artifact).unwrap(), "already\n");

        let notes = dir.path().join("notes.txt");
        let error = write_new_session(&session, None, None, None, Some(&notes), &Config::default())
            .unwrap_err();
        assert!(error.to_string().contains(".md"), "{error}");
        assert!(!session.exists());
        assert!(!notes.exists());
    }
}
