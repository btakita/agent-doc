//! `agent-doc convert` — Bidirectional mode conversion between append and template.
//!
//! Usage: agent-doc convert <FILE> [MODE]
//!
//! - Append → Template: wraps content from first `## User` in `<!-- agent:exchange -->` markers
//! - Template → Append: strips component markers and `## Exchange` headings, preserving content

use anyhow::{Context, Result};
use std::path::Path;

use crate::{frontmatter, snapshot, write, AgentDocMode};

pub fn run(file: &Path, target_mode: &AgentDocMode) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;

    let (fm, body) = frontmatter::parse(&content)?;
    let current_mode_str = fm.mode.clone().unwrap_or_else(|| "append".to_string());

    match target_mode {
        AgentDocMode::Template => convert_to_template(file, &content, fm, body, &current_mode_str),
        AgentDocMode::Append => convert_to_append(file, fm, body, &current_mode_str),
    }
}

fn convert_to_template(
    file: &Path,
    content: &str,
    fm: frontmatter::Frontmatter,
    body: &str,
    current_mode: &str,
) -> Result<()> {
    if current_mode == "template" {
        let components = crate::component::parse(content).unwrap_or_default();
        if !components.is_empty() {
            anyhow::bail!("{} is already in template mode with components", file.display());
        }
        eprintln!("Mode is template but no component markers found, adding exchange component");
    }

    let mut fm = fm;
    fm.mode = Some("template".to_string());

    let exchange_content = append_to_template_body(body);
    let new_doc = frontmatter::write(&fm, &exchange_content)?;

    write::atomic_write_pub(file, &new_doc)?;
    snapshot::save(file, &new_doc)?;

    eprintln!("Converted {} to template mode", file.display());
    Ok(())
}

fn convert_to_append(
    file: &Path,
    fm: frontmatter::Frontmatter,
    body: &str,
    current_mode: &str,
) -> Result<()> {
    if current_mode == "append" {
        anyhow::bail!("{} is already in append mode", file.display());
    }

    let mut fm = fm;
    fm.mode = Some("append".to_string());

    let append_content = template_to_append_body(body);
    let new_doc = frontmatter::write(&fm, &append_content)?;

    write::atomic_write_pub(file, &new_doc)?;
    snapshot::save(file, &new_doc)?;

    eprintln!("Converted {} to append mode", file.display());
    Ok(())
}

/// Convert append-mode body to template-mode body.
/// Finds the first ## User heading and wraps from there to end in <!-- agent:exchange -->.
/// Content before the first ## User is preserved as-is.
fn append_to_template_body(body: &str) -> String {
    // Find first ## User heading (not inside code blocks)
    let lines: Vec<&str> = body.lines().collect();
    let mut in_code_block = false;
    let mut first_user_line = None;

    for (i, line) in lines.iter().enumerate() {
        if line.starts_with("```") || line.starts_with("~~~") {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block {
            continue;
        }
        if line.starts_with("## User") {
            first_user_line = Some(i);
            break;
        }
    }

    match first_user_line {
        Some(idx) => {
            let before = lines[..idx].join("\n");
            let exchange = lines[idx..].join("\n");
            let mut result = before;
            if !result.is_empty() && !result.ends_with('\n') {
                result.push('\n');
            }
            // Add exchange component with ## Exchange heading
            result.push_str("\n## Exchange\n\n<!-- agent:exchange -->\n");
            result.push_str(&exchange);
            if !result.ends_with('\n') {
                result.push('\n');
            }
            result.push_str("<!-- /agent:exchange -->\n");
            result
        }
        None => {
            // No User blocks found — just add empty exchange component
            let mut result = body.to_string();
            if !result.is_empty() && !result.ends_with('\n') {
                result.push('\n');
            }
            result.push_str("\n## Exchange\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n");
            result
        }
    }
}

/// Convert template-mode body to append-mode body.
/// Strips component markers and ## Exchange heading, preserving content.
fn template_to_append_body(body: &str) -> String {
    let mut result = String::new();
    let lines: Vec<&str> = body.lines().collect();
    let mut in_code_block = false;
    let mut skip_exchange_heading = false;

    for (i, line) in lines.iter().enumerate() {
        if line.starts_with("```") || line.starts_with("~~~") {
            in_code_block = !in_code_block;
        }

        if in_code_block {
            result.push_str(line);
            result.push('\n');
            continue;
        }

        // Skip component markers (<!-- agent:name --> and <!-- /agent:name -->)
        let trimmed = line.trim();
        if (trimmed.starts_with("<!-- agent:") && trimmed.ends_with("-->"))
            || (trimmed.starts_with("<!-- /agent:") && trimmed.ends_with("-->"))
        {
            // If the next non-empty line after an opening marker starts content,
            // we just skip the marker line itself
            continue;
        }

        // Skip ## Exchange heading (and its trailing blank line)
        if trimmed == "## Exchange" {
            skip_exchange_heading = true;
            continue;
        }
        if skip_exchange_heading && trimmed.is_empty() {
            skip_exchange_heading = false;
            // Skip the blank line after ## Exchange only if the next content
            // isn't also blank (avoid eating real content spacing)
            if i + 1 < lines.len() {
                continue;
            }
        }
        skip_exchange_heading = false;

        result.push_str(line);
        result.push('\n');
    }

    // Trim trailing excess newlines but keep one
    let trimmed = result.trim_end_matches('\n');
    let mut final_result = trimmed.to_string();
    final_result.push('\n');
    final_result
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_project() -> TempDir {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        dir
    }

    #[test]
    fn convert_body_with_user_blocks() {
        let body = "\n# Session: Test\n\n## User\n\nHello\n\n## Assistant\n\nHi there\n\n## User\n\n";
        let result = append_to_template_body(body);
        assert!(result.contains("<!-- agent:exchange -->"));
        assert!(result.contains("<!-- /agent:exchange -->"));
        assert!(result.contains("## User"));
        assert!(result.contains("# Session: Test"));
        // Title should be outside exchange
        let exchange_start = result.find("<!-- agent:exchange -->").unwrap();
        let title_pos = result.find("# Session: Test").unwrap();
        assert!(title_pos < exchange_start);
    }

    #[test]
    fn convert_body_no_user_blocks() {
        let body = "\n# Just a doc\n\nSome content.\n";
        let result = append_to_template_body(body);
        assert!(result.contains("<!-- agent:exchange -->"));
        assert!(result.contains("<!-- /agent:exchange -->"));
    }

    #[test]
    fn convert_rejects_template_mode_with_components() {
        let dir = setup_project();
        let file = dir.path().join("test.md");
        std::fs::write(&file, "---\nagent_doc_mode: template\n---\n\n<!-- agent:exchange -->\ncontent\n<!-- /agent:exchange -->\n").unwrap();
        let result = run(&file, &AgentDocMode::Template);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already in template mode"));
    }

    #[test]
    fn convert_adds_markers_when_template_but_no_components() {
        let dir = setup_project();
        let file = dir.path().join("test.md");
        std::fs::write(&file, "---\nagent_doc_mode: template\n---\n\n# Doc\n\n## User\n\nHello\n").unwrap();
        run(&file, &AgentDocMode::Template).unwrap();
        let result = std::fs::read_to_string(&file).unwrap();
        assert!(result.contains("<!-- agent:exchange -->"));
        assert!(result.contains("Hello"));
    }

    #[test]
    fn convert_updates_frontmatter() {
        let dir = setup_project();
        let file = dir.path().join("test.md");
        std::fs::write(&file, "---\nagent_doc_session: abc-123\nagent: claude\n---\n\n# Session: Test\n\n## User\n\nHello\n").unwrap();
        run(&file, &AgentDocMode::Template).unwrap();
        let result = std::fs::read_to_string(&file).unwrap();
        assert!(result.contains("agent_doc_mode: template"));
        assert!(result.contains("<!-- agent:exchange -->"));
        assert!(result.contains("Hello"));
    }

    #[test]
    fn convert_body_preserves_code_blocks() {
        let body = "\n# Doc\n\n```\n## User\n```\n\n## User\n\nReal user block\n";
        let result = append_to_template_body(body);
        // The exchange should start at the REAL ## User, not the one inside code block
        let exchange_start = result.find("<!-- agent:exchange -->").unwrap();
        let code_block_pos = result.find("```\n## User\n```").unwrap();
        // Code block should be before exchange
        assert!(code_block_pos < exchange_start);
    }

    #[test]
    fn convert_to_append_strips_markers() {
        let body = "\n# Doc\n\n## Exchange\n\n<!-- agent:exchange -->\n## User\n\nHello\n\n## Assistant\n\nHi\n<!-- /agent:exchange -->\n";
        let result = template_to_append_body(body);
        assert!(!result.contains("<!-- agent:exchange -->"));
        assert!(!result.contains("<!-- /agent:exchange -->"));
        assert!(!result.contains("## Exchange"));
        assert!(result.contains("## User"));
        assert!(result.contains("Hello"));
        assert!(result.contains("## Assistant"));
        assert!(result.contains("Hi"));
    }

    #[test]
    fn convert_to_append_rejects_already_append() {
        let dir = setup_project();
        let file = dir.path().join("test.md");
        std::fs::write(&file, "---\nagent_doc_mode: append\n---\n\n## User\n\nHello\n").unwrap();
        let result = run(&file, &AgentDocMode::Append);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already in append mode"));
    }

    #[test]
    fn convert_roundtrip_append_to_template_to_append() {
        let dir = setup_project();
        let file = dir.path().join("test.md");
        let original = "---\nagent_doc_session: abc-123\n---\n\n# Session\n\n## User\n\nHello\n\n## Assistant\n\nWorld\n";
        std::fs::write(&file, original).unwrap();

        // append -> template
        run(&file, &AgentDocMode::Template).unwrap();
        let template = std::fs::read_to_string(&file).unwrap();
        assert!(template.contains("agent_doc_mode: template"));
        assert!(template.contains("<!-- agent:exchange -->"));

        // template -> append
        run(&file, &AgentDocMode::Append).unwrap();
        let append = std::fs::read_to_string(&file).unwrap();
        assert!(append.contains("agent_doc_mode: append"));
        assert!(!append.contains("<!-- agent:exchange -->"));
        assert!(append.contains("## User"));
        assert!(append.contains("Hello"));
        assert!(append.contains("## Assistant"));
        assert!(append.contains("World"));
    }

    #[test]
    fn template_to_append_preserves_non_exchange_content() {
        let body = "\n# Doc\n\n<!-- agent:status -->\nStatus line\n<!-- /agent:status -->\n\n## Exchange\n\n<!-- agent:exchange -->\nConversation\n<!-- /agent:exchange -->\n";
        let result = template_to_append_body(body);
        // All markers should be stripped
        assert!(!result.contains("<!-- agent:"));
        assert!(!result.contains("<!-- /agent:"));
        // Content preserved
        assert!(result.contains("Status line"));
        assert!(result.contains("Conversation"));
    }
}
