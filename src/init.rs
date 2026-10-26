//! # Module: init
//!
//! ## Spec
//! - `run(file, title, agent, mode, config)`: dispatches on whether a file path was provided.
//!   - No file: calls `init_project()` — checks prerequisites, creates `.agent-doc/`, and installs
//!     `SKILL.md` for the detected harness (generic `.agent/skills/` when none is detected, in
//!     which case it prints the `skill install --harness` command to run), then prints next
//!     steps ending in the README "Getting started" link. Per-document state is stored in the typed ledger, not a filesystem sidecar.
//!   - With file: calls `init_file()` — lazily runs project init if `.agent-doc/` is absent, then
//!     scaffolds a new session document.  Fails if the file already exists.
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

use anyhow::Result;
use std::path::Path;
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
    let content = format!(
        "---\nagent_doc_session: {session_id}\nagent: {agent}\nagent_doc_format: template\nagent_doc_write: crdt\nenable_tool_search: true\n---\n\n# {title}\n\n## Exchange\n\n<!-- agent:exchange -->\n{initial}<!-- /agent:exchange -->\n"
    );
    std::fs::write(file, content)?;
    eprintln!("Initialized {}", file.display());
    Ok(SessionPreparation::Initialized)
}

/// Initialize a project (no file given): check prereqs, create .agent-doc/, install skill.
fn init_project() -> Result<()> {
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
    config: &Config,
) -> Result<()> {
    // Lazy project init: if .agent-doc/ doesn't exist, run project init first.
    if !Path::new(".agent-doc").exists() {
        eprintln!("[init] No .agent-doc/ found — running project init first.");
        init_project()?;
    }

    if file.exists() {
        anyhow::bail!("file already exists: {}", file.display());
    }

    let title = title.unwrap_or("Untitled Session");
    let agent = agent
        .or(config.default_agent.as_deref())
        .unwrap_or("claude");
    let session_id = Uuid::new_v4();
    let mode = mode.unwrap_or("append");

    let content = if mode == "template" || mode == "stream" {
        format!(
            "---\nagent_doc_session: {}\nagent: {}\nagent_doc_format: template\nagent_doc_write: crdt\nenable_tool_search: true\n---\n\n# {}\n\n## Exchange\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n",
            session_id, agent, title
        )
    } else {
        format!(
            "---\nagent_doc_session: {}\nagent: {}\n---\n\n# Session: {}\n\n## User\n\n",
            session_id, agent, title
        )
    };

    if let Some(parent) = file.parent()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(file, content)?;
    eprintln!("Created {}", file.display());
    Ok(())
}

pub fn run(
    file: Option<&Path>,
    title: Option<&str>,
    agent: Option<&str>,
    mode: Option<&str>,
    config: &Config,
) -> Result<()> {
    match file {
        None => init_project(),
        Some(path) => init_file(path, title, agent, mode, config),
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
        assert!(content.contains("<!-- agent:exchange -->\nKeep this prompt.\n"));
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
}
