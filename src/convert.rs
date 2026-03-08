use anyhow::{Context, Result};
use std::path::Path;

use crate::{frontmatter, snapshot, write};

pub fn run(file: &Path) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;

    let (fm, body) = frontmatter::parse(&content)?;

    // Reject if already template mode
    let mode = fm.mode.as_deref().unwrap_or("append");
    if mode == "template" {
        anyhow::bail!("{} is already in template mode", file.display());
    }

    // Update frontmatter to template mode
    let mut fm = fm;
    fm.mode = Some("template".to_string());

    // Find the first ## User heading and wrap everything from there in exchange component
    let exchange_content = convert_body(body);
    let new_doc = frontmatter::write(&fm, &exchange_content)?;

    // Atomic write
    write::atomic_write_pub(file, &new_doc)?;

    // Update snapshot
    snapshot::save(file, &new_doc)?;

    eprintln!("Converted {} to template mode", file.display());
    Ok(())
}

/// Convert append-mode body to template-mode body.
/// Finds the first ## User heading and wraps from there to end in <!-- agent:exchange -->.
/// Content before the first ## User is preserved as-is.
fn convert_body(body: &str) -> String {
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
        let result = convert_body(body);
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
        let result = convert_body(body);
        assert!(result.contains("<!-- agent:exchange -->"));
        assert!(result.contains("<!-- /agent:exchange -->"));
    }

    #[test]
    fn convert_rejects_template_mode() {
        let dir = setup_project();
        let file = dir.path().join("test.md");
        std::fs::write(&file, "---\nagent_doc_mode: template\n---\n\n# Doc\n").unwrap();
        let result = run(&file);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already in template mode"));
    }

    #[test]
    fn convert_updates_frontmatter() {
        let dir = setup_project();
        let file = dir.path().join("test.md");
        std::fs::write(&file, "---\nagent_doc_session: abc-123\nagent: claude\n---\n\n# Session: Test\n\n## User\n\nHello\n").unwrap();
        run(&file).unwrap();
        let result = std::fs::read_to_string(&file).unwrap();
        assert!(result.contains("agent_doc_mode: template"));
        assert!(result.contains("<!-- agent:exchange -->"));
        assert!(result.contains("Hello"));
    }

    #[test]
    fn convert_body_preserves_code_blocks() {
        let body = "\n# Doc\n\n```\n## User\n```\n\n## User\n\nReal user block\n";
        let result = convert_body(body);
        // The exchange should start at the REAL ## User, not the one inside code block
        let exchange_start = result.find("<!-- agent:exchange -->").unwrap();
        let code_block_pos = result.find("```\n## User\n```").unwrap();
        // Code block should be before exchange
        assert!(code_block_pos < exchange_start);
    }
}
