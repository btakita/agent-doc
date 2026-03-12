use anyhow::Result;
use std::path::Path;
use uuid::Uuid;

use crate::config::Config;

pub fn run(
    file: &Path,
    title: Option<&str>,
    agent: Option<&str>,
    mode: Option<&str>,
    config: &Config,
) -> Result<()> {
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
