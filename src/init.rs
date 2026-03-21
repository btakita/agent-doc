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
    eprintln!("[init]   agent-doc watch             # watch for changes and auto-submit");

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
            "---\nagent_doc_session: {}\nagent: {}\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n# {}\n\n## Exchange\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n",
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
