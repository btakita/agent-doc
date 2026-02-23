use anyhow::Result;
use std::path::Path;

use crate::{agent, config::Config, diff, frontmatter, git, snapshot};

pub fn run(
    file: &Path,
    branch: bool,
    agent_name: Option<&str>,
    model: Option<&str>,
    dry_run: bool,
    no_git: bool,
    config: &Config,
) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    // Compute diff
    let the_diff = match diff::compute(file)? {
        Some(d) => d,
        None => {
            eprintln!("Nothing changed since last submit.");
            return Ok(());
        }
    };

    let content = std::fs::read_to_string(file)?;
    let (fm, _body) = frontmatter::parse(&content)?;

    // Resolve agent
    let agent_name = agent_name
        .or(fm.agent.as_deref())
        .or(config.default_agent.as_deref())
        .unwrap_or("claude");
    let agent_config = config.agents.get(agent_name);
    let backend = agent::resolve(agent_name, agent_config)?;

    // Build prompt
    let prompt = if fm.session.is_some() {
        format!(
            "The user edited the session document. Here is the diff since the last submit:\n\n\
             <diff>\n{}\n</diff>\n\n\
             The full document is now:\n\n\
             <document>\n{}\n</document>\n\n\
             Respond to the user's new content. Write your response in markdown.\n\
             Do not include a ## Assistant heading — it will be added automatically.\n\
             If the user asked questions inline (e.g., in blockquotes), address those too.",
            the_diff, content
        )
    } else {
        format!(
            "The user is starting a session document. Here is the full document:\n\n\
             <document>\n{}\n</document>\n\n\
             Respond to the user's content. Write your response in markdown.\n\
             Do not include a ## Assistant heading — it will be added automatically.\n\
             If the user asked questions inline (e.g., in blockquotes), address those too.",
            content
        )
    };

    if dry_run {
        eprintln!("--- Diff ---");
        print!("{}", the_diff);
        eprintln!("--- Prompt would be {} bytes ---", prompt.len());
        return Ok(());
    }

    // Create branch if requested
    if branch && !no_git {
        git::create_branch(file)?;
    }

    eprintln!("Submitting to {}...", agent_name);

    // Send to agent
    let fork = fm.session.is_none();
    let model = model.or(fm.model.as_deref());
    let response = backend.send(&prompt, fm.session.as_deref(), fork, model)?;

    // Update session ID in frontmatter
    let mut content = std::fs::read_to_string(file)?;
    if let Some(ref sid) = response.session_id {
        content = frontmatter::set_session_id(&content, sid)?;
    }

    // Append response
    content.push_str("\n## Assistant\n\n");
    content.push_str(&response.text);
    content.push_str("\n\n## User\n\n");

    std::fs::write(file, &content)?;

    // Save snapshot
    snapshot::save(file, &content)?;

    // Git commit
    if !no_git {
        git::commit(file)?;
    }

    eprintln!("Response appended to {}", file.display());
    Ok(())
}
