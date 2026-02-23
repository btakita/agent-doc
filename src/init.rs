use anyhow::Result;
use std::path::Path;

pub fn run(file: &Path, title: Option<&str>, agent: Option<&str>) -> Result<()> {
    if file.exists() {
        anyhow::bail!("file already exists: {}", file.display());
    }

    let title = title.unwrap_or("Untitled Session");
    let agent = agent.unwrap_or("claude");

    let content = format!(
        "---\nsession:\nagent: {}\n---\n\n# Session: {}\n\n## User\n\n",
        agent, title
    );

    if let Some(parent) = file.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(file, content)?;
    eprintln!("Created {}", file.display());
    Ok(())
}
