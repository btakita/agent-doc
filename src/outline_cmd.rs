use anyhow::Result;
use std::path::Path;

pub fn run_outline(file: &Path, json: bool) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    let content = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "outline_command_document",
    )?;
    let (_fm, body) = agent_doc_frontmatter::frontmatter::parse(&content)?;
    let sections = agent_doc_document::outline_projection::project_markdown_outline(body);

    if json {
        println!("{}", serde_json::to_string(&sections)?);
    } else {
        print!(
            "{}",
            agent_doc_document::outline_projection::render_markdown_outline_text(&sections)
        );
    }

    Ok(())
}
