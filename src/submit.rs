use anyhow::Result;
use std::path::Path;
use std::process::Command;

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

    // Ensure the document has a session UUID (for tmux routing)
    let raw_content = std::fs::read_to_string(file)?;
    let (content_original, _session_id) = frontmatter::ensure_session(&raw_content)?;
    if content_original != raw_content {
        std::fs::write(file, &content_original)?;
    }
    let (fm, _body) = frontmatter::parse(&content_original)?;

    // Resolve agent
    let agent_name = agent_name
        .or(fm.agent.as_deref())
        .or(config.default_agent.as_deref())
        .unwrap_or("claude");
    let agent_config = config.agents.get(agent_name);
    let backend = agent::resolve(agent_name, agent_config)?;

    // Build prompt
    let prompt = if fm.resume.is_some() {
        format!(
            "The user edited the session document. Here is the diff since the last submit:\n\n\
             <diff>\n{}\n</diff>\n\n\
             The full document is now:\n\n\
             <document>\n{}\n</document>\n\n\
             Respond to the user's new content. Write your response in markdown.\n\
             Do not include a ## Assistant heading — it will be added automatically.\n\
             If the user asked questions inline (e.g., in blockquotes), address those too.",
            the_diff, content_original
        )
    } else {
        format!(
            "The user is starting a session document. Here is the full document:\n\n\
             <document>\n{}\n</document>\n\n\
             Respond to the user's content. Write your response in markdown.\n\
             Do not include a ## Assistant heading — it will be added automatically.\n\
             If the user asked questions inline (e.g., in blockquotes), address those too.",
            content_original
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

    // Pre-commit: commit user's changes before sending to agent
    // This lets the editor show agent additions as diff gutters
    if !no_git {
        git::commit(file)?;
    }

    eprintln!("Submitting to {}...", agent_name);

    // Send to agent — use `resume` for agent conversation tracking
    let fork = fm.resume.is_none();
    let model = model.or(fm.model.as_deref());
    let response = backend.send(&prompt, fm.resume.as_deref(), fork, model)?;

    // Build our version: original + resume_id update + response appended
    let mut content_ours = content_original.clone();
    if let Some(ref sid) = response.session_id {
        content_ours = frontmatter::set_resume_id(&content_ours, sid)?;
    }
    content_ours.push_str("\n## Assistant\n\n");
    content_ours.push_str(&response.text);
    content_ours.push_str("\n\n## User\n\n");

    // Re-read file to check for user edits during submit
    let content_current = std::fs::read_to_string(file)?;

    let final_content = if content_current == content_original {
        // No edits during submit — use our version directly
        content_ours
    } else {
        eprintln!("File was modified during submit. Merging changes...");
        merge_contents(&content_original, &content_ours, &content_current)?
    };

    std::fs::write(file, &final_content)?;

    // Save snapshot (but don't commit — leave agent response as uncommitted
    // so the editor shows diff gutters for what the agent added)
    snapshot::save(file, &final_content)?;

    eprintln!("Response appended to {}", file.display());
    Ok(())
}

/// 3-way merge using git merge-file.
/// base = original content, ours = original + response, theirs = user's edits.
/// Returns merged content (with conflict markers if conflicts exist).
fn merge_contents(base: &str, ours: &str, theirs: &str) -> Result<String> {
    let tmp = std::env::temp_dir().join(format!("agent-doc-merge-{}", std::process::id()));
    std::fs::create_dir_all(&tmp)?;

    let base_path = tmp.join("base");
    let ours_path = tmp.join("ours");
    let theirs_path = tmp.join("theirs");

    std::fs::write(&base_path, base)?;
    std::fs::write(&ours_path, ours)?;
    std::fs::write(&theirs_path, theirs)?;

    // git merge-file -p writes merged result to stdout
    // exit 0 = clean merge, 1 = conflicts, <0 = error
    let output = Command::new("git")
        .args([
            "merge-file",
            "-p",
            "--diff3",
            "-L", "agent-response",
            "-L", "original",
            "-L", "your-edits",
            &ours_path.to_string_lossy(),
            &base_path.to_string_lossy(),
            &theirs_path.to_string_lossy(),
        ])
        .output()?;

    // Clean up temp files
    let _ = std::fs::remove_dir_all(&tmp);

    let merged = String::from_utf8(output.stdout)
        .map_err(|e| anyhow::anyhow!("merge produced invalid UTF-8: {}", e))?;

    if output.status.success() {
        eprintln!("Merge successful — user edits preserved.");
    } else if output.status.code() == Some(1) {
        eprintln!("WARNING: Merge conflicts detected. Please resolve conflict markers manually.");
    } else {
        anyhow::bail!(
            "git merge-file failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(merged)
}
