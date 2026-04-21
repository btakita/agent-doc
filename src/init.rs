//! # Module: init
//!
//! ## Spec
//! - `run(file, title, agent, mode, config)`: dispatches on whether a file path was provided.
//!   - No file: calls `init_project()` — checks prerequisites, creates `.agent-doc/snapshots/` and
//!     `.agent-doc/patches/`, and installs `SKILL.md`.
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
//! - init_project_creates_dirs: `.agent-doc/snapshots` and `.agent-doc/patches` are created → both dirs exist
//! - init_file_template_mode: `mode="template"` → scaffolded file contains `agent_doc_format: template` and `agent:exchange` component
//! - init_file_append_mode: `mode="append"` → scaffolded file contains `## User` block, no exchange component
//! - init_file_already_exists: target file exists → returns Err

use anyhow::Result;
use std::path::Path;
use uuid::Uuid;

use crate::config::Config;

/// Initialize a project (no file given): check prereqs, create .agent-doc/, install skill.
fn init_project() -> Result<()> {
    // Check prerequisites
    crate::install::check_prereqs();

    // Create .agent-doc/ directory structure
    let agent_doc_dir = Path::new(".agent-doc");
    for subdir in &["snapshots", "patches"] {
        let dir = agent_doc_dir.join(subdir);
        std::fs::create_dir_all(&dir)?;
        eprintln!("[init] Created {}", dir.display());
    }

    // Install SKILL.md
    crate::skill::install_and_check_updated()?;
    eprintln!("[init] Installed .claude/skills/agent-doc/SKILL.md");

    eprintln!("[init] Project initialized. Quick start:");
    eprintln!("[init]   agent-doc init <file.md>   # scaffold a session document");
    eprintln!("[init]   agent-doc run <file.md>     # run a session");
    eprintln!("[init]   agent-doc watch             # watch for changes and auto-run");

    Ok(())
}

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
