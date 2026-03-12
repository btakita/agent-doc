//! `agent-doc compact` — Archive old exchanges and compact components.
//!
//! Usage: agent-doc compact <file.md> [--keep N] [--component NAME] [--message MSG]
//!
//! **Append mode:** Moves old User/Assistant exchange pairs to an archive file
//! under `.agent-doc/archives/`, leaving only the most recent N exchanges.
//!
//! **Template/stream mode:** Replaces the content of a named component
//! (default: `exchange`) with a summary marker. Archives old content.

use anyhow::{Context, Result};
use std::path::Path;

use crate::{component, frontmatter, snapshot};

/// A parsed exchange pair (User prompt + Assistant response).
#[derive(Debug)]
struct Exchange {
    /// The user's content (without the `## User` heading)
    user: String,
    /// The assistant's content (without the `## Assistant` heading)
    assistant: String,
}

/// Run the compact command.
///
/// `keep` is the number of recent exchanges to keep in the document.
/// `component_name` targets a specific component in template/stream mode.
/// `message` is the summary marker text (default: auto-generated).
pub fn run(
    file: &Path,
    keep: usize,
    component_name: Option<&str>,
    message: Option<&str>,
) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;

    let (fm, body) = frontmatter::parse(&content)?;

    let mode = fm.mode.as_deref().unwrap_or("append");
    match mode {
        "template" | "stream" => {
            // Compact CRDT state if stream mode
            if mode == "stream"
                && let Ok(Some(crdt_state)) = snapshot::load_crdt(file)
            {
                let compacted_crdt = crate::crdt::compact(&crdt_state)?;
                snapshot::save_crdt(file, &compacted_crdt)?;
                eprintln!(
                    "[compact] CRDT state compacted: {} → {} bytes",
                    crdt_state.len(),
                    compacted_crdt.len()
                );
            }

            let target = component_name.unwrap_or("exchange");
            return run_component_compact(file, &content, target, message);
        }
        _ => {} // append mode — continue
    }

    // Parse exchanges from the body
    let exchanges = parse_exchanges(body);

    if exchanges.len() <= keep {
        eprintln!(
            "[compact] Only {} exchange(s) found, keeping all (threshold: {})",
            exchanges.len(),
            keep
        );
        return Ok(());
    }

    let to_archive = &exchanges[..exchanges.len() - keep];
    let to_keep = &exchanges[exchanges.len() - keep..];

    // Build archive content
    let archive_content = build_archive(&content, to_archive);

    // Save archive
    let archive_path = save_archive(file, &archive_content)?;

    // Build compacted document
    let compacted = build_compacted(&content, body, to_keep, &archive_path, to_archive.len());

    // Atomic write
    crate::write::atomic_write_pub(file, &compacted)?;

    // Update snapshot
    snapshot::save(file, &compacted)?;

    eprintln!(
        "[compact] Archived {} exchange(s) to {}",
        to_archive.len(),
        archive_path.display()
    );
    eprintln!(
        "[compact] {} exchange(s) remain in {}",
        to_keep.len(),
        file.display()
    );

    Ok(())
}

/// Compact a named component in a template/stream-mode document.
///
/// Archives the component content and replaces it with a summary marker.
/// Single atomic write — no intermediate state.
fn run_component_compact(
    file: &Path,
    content: &str,
    target: &str,
    message: Option<&str>,
) -> Result<()> {
    let components = component::parse(content)?;
    let comp = components
        .iter()
        .find(|c| c.name == target)
        .ok_or_else(|| anyhow::anyhow!("component '{}' not found in document", target))?;

    let old_content = comp.content(content);
    let trimmed = old_content.trim();

    if trimmed.is_empty() {
        eprintln!("[compact] Component '{}' is already empty", target);
        return Ok(());
    }

    // Archive old content
    let archive_path = save_archive(file, &build_component_archive(content, target, old_content))?;

    // Build summary marker
    let summary = match message {
        Some(msg) => format!("{}\n", msg),
        None => format!(
            "*Compacted. Content archived to `{}`*\n",
            archive_path.display()
        ),
    };

    // Single atomic write: replace component content + update snapshot
    let compacted = comp.replace_content(content, &summary);
    crate::write::atomic_write_pub(file, &compacted)?;
    snapshot::save(file, &compacted)?;

    let line_count = old_content.lines().count();
    eprintln!(
        "[compact] Archived {} lines from component '{}' to {}",
        line_count,
        target,
        archive_path.display()
    );

    Ok(())
}

/// Build archive content from a component.
fn build_component_archive(original: &str, component_name: &str, content: &str) -> String {
    let mut archive = String::new();

    archive.push_str("---\n");
    archive.push_str("archived_from: compact\n");
    archive.push_str(&format!("archived_at: {}\n", chrono_timestamp()));
    archive.push_str(&format!("component: {}\n", component_name));

    if let Ok((fm, _)) = frontmatter::parse(original)
        && let Some(session) = &fm.session
    {
        archive.push_str(&format!("session: {}\n", session));
    }

    archive.push_str("---\n\n");
    archive.push_str(content.trim());
    archive.push('\n');

    archive
}

/// Parse the document body into User/Assistant exchange pairs.
fn parse_exchanges(body: &str) -> Vec<Exchange> {
    let mut exchanges = Vec::new();
    let mut sections: Vec<(&str, String)> = Vec::new(); // (type, content)

    // Split by ## User and ## Assistant headings
    let mut current_type = "";
    let mut current_content = String::new();
    let mut in_code_block = false;

    for line in body.lines() {
        // Track code blocks to avoid matching headings inside them
        if line.starts_with("```") {
            in_code_block = !in_code_block;
        }

        if !in_code_block {
            if line == "## User" {
                if !current_type.is_empty() {
                    sections.push((current_type, current_content.clone()));
                }
                current_type = "user";
                current_content.clear();
                continue;
            } else if line == "## Assistant" {
                if !current_type.is_empty() {
                    sections.push((current_type, current_content.clone()));
                }
                current_type = "assistant";
                current_content.clear();
                continue;
            }
        }

        if !current_type.is_empty() {
            current_content.push_str(line);
            current_content.push('\n');
        }
    }

    // Push last section
    if !current_type.is_empty() {
        sections.push((current_type, current_content));
    }

    // Pair up User + Assistant sections into exchanges
    let mut i = 0;
    while i < sections.len() {
        if sections[i].0 == "user" {
            let user = sections[i].1.trim().to_string();
            let assistant = if i + 1 < sections.len() && sections[i + 1].0 == "assistant" {
                i += 1;
                sections[i].1.trim().to_string()
            } else {
                String::new()
            };
            // Only include complete exchanges (with assistant response)
            if !assistant.is_empty() {
                exchanges.push(Exchange { user, assistant });
            }
            // If no assistant response, this is the active/pending user block — skip it
        }
        i += 1;
    }

    exchanges
}

/// Build archive file content from exchanges.
fn build_archive(original_header: &str, exchanges: &[Exchange]) -> String {
    let mut archive = String::new();

    // Add a header noting the source
    archive.push_str("---\n");
    archive.push_str("archived_from: compact\n");
    archive.push_str(&format!(
        "archived_at: {}\n",
        chrono_timestamp()
    ));
    archive.push_str(&format!("exchange_count: {}\n", exchanges.len()));

    // Preserve original frontmatter session ID if present
    if let Ok((fm, _)) = frontmatter::parse(original_header)
        && let Some(session) = &fm.session
    {
        archive.push_str(&format!("session: {}\n", session));
    }

    archive.push_str("---\n\n");

    for (i, exchange) in exchanges.iter().enumerate() {
        archive.push_str("## User\n\n");
        archive.push_str(&exchange.user);
        archive.push('\n');
        archive.push_str("\n## Assistant\n\n");
        archive.push_str(&exchange.assistant);
        archive.push('\n');
        if i < exchanges.len() - 1 {
            archive.push('\n');
        }
    }

    archive
}

/// Save archive to `.agent-doc/archives/<hash>-<timestamp>.md`.
fn save_archive(doc: &Path, content: &str) -> Result<std::path::PathBuf> {
    let snap_path = snapshot::path_for(doc)?;
    // Extract the hash from snapshot path (filename without .md)
    let hash = snap_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");

    // Build archive dir relative to project root
    let project_root = find_project_root(doc)?;
    let archive_dir = project_root.join(".agent-doc/archives");
    std::fs::create_dir_all(&archive_dir)
        .with_context(|| format!("failed to create {}", archive_dir.display()))?;

    let timestamp = chrono_timestamp();
    let archive_path = archive_dir.join(format!("{}-{}.md", hash, timestamp));

    std::fs::write(&archive_path, content)
        .with_context(|| format!("failed to write {}", archive_path.display()))?;

    Ok(archive_path)
}

/// Build the compacted document content.
fn build_compacted(
    original: &str,
    body: &str,
    kept_exchanges: &[Exchange],
    archive_path: &Path,
    archived_count: usize,
) -> String {
    // Extract frontmatter (everything before the body)
    let body_start = original.len() - body.len();
    let header = &original[..body_start];

    let mut result = header.to_string();

    // Add archive summary
    result.push_str(&format!(
        "*{} earlier exchange(s) archived to `{}`*\n\n",
        archived_count,
        archive_path.display()
    ));

    // Add kept exchanges
    for exchange in kept_exchanges {
        result.push_str("## User\n\n");
        result.push_str(&exchange.user);
        result.push_str("\n\n## Assistant\n\n");
        result.push_str(&exchange.assistant);
        result.push_str("\n\n");
    }

    // Add trailing ## User for next prompt
    result.push_str("## User\n\n");

    result
}

/// Find project root by walking up to find `.agent-doc/`.
fn find_project_root(file: &Path) -> Result<std::path::PathBuf> {
    let canonical = file
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", file.display()))?;
    let mut dir = canonical.parent();
    while let Some(d) = dir {
        if d.join(".agent-doc").is_dir() {
            return Ok(d.to_path_buf());
        }
        dir = d.parent();
    }
    // Fallback to file's parent
    Ok(canonical
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf())
}

/// Generate a compact timestamp for archive filenames.
fn chrono_timestamp() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    // Format as YYYYMMDD-HHMMSS
    let secs = now.as_secs();
    // Simple UTC timestamp without chrono dependency
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Days since epoch to date (simplified)
    let mut y = 1970i64;
    let mut remaining_days = days as i64;
    loop {
        let days_in_year = if is_leap_year(y) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        y += 1;
    }
    let month_days: &[i64] = if is_leap_year(y) {
        &[31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        &[31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut m = 0;
    for &md in month_days {
        if remaining_days < md {
            break;
        }
        remaining_days -= md;
        m += 1;
    }

    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        y,
        m + 1,
        remaining_days + 1,
        hours,
        minutes,
        seconds
    )
}

fn is_leap_year(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_exchanges_basic() {
        let body = "## User\n\nHello\n\n## Assistant\n\nHi there\n\n## User\n\nBye\n\n## Assistant\n\nGoodbye\n\n## User\n\n";
        let exchanges = parse_exchanges(body);
        assert_eq!(exchanges.len(), 2);
        assert_eq!(exchanges[0].user, "Hello");
        assert_eq!(exchanges[0].assistant, "Hi there");
        assert_eq!(exchanges[1].user, "Bye");
        assert_eq!(exchanges[1].assistant, "Goodbye");
    }

    #[test]
    fn parse_exchanges_with_code_blocks() {
        let body = "## User\n\nHere's code:\n\n```\n## User\n## Assistant\n```\n\n## Assistant\n\nNice code\n\n## User\n\n";
        let exchanges = parse_exchanges(body);
        assert_eq!(exchanges.len(), 1);
        assert!(exchanges[0].user.contains("```"));
        assert!(exchanges[0].user.contains("## User"));
    }

    #[test]
    fn parse_exchanges_trailing_user_not_counted() {
        let body = "## User\n\nHello\n\n## Assistant\n\nHi\n\n## User\n\nPending question\n";
        let exchanges = parse_exchanges(body);
        // Only the complete exchange is counted, not the trailing User block
        assert_eq!(exchanges.len(), 1);
    }

    #[test]
    fn build_archive_format() {
        let exchanges = vec![Exchange {
            user: "Hello".to_string(),
            assistant: "Hi there".to_string(),
        }];
        let archive = build_archive("---\nsession: test\n---\n", &exchanges);
        assert!(archive.contains("archived_from: compact"));
        assert!(archive.contains("session: test"));
        assert!(archive.contains("## User\n\nHello"));
        assert!(archive.contains("## Assistant\n\nHi there"));
    }

    #[test]
    fn build_compacted_format() {
        let kept = vec![Exchange {
            user: "Recent question".to_string(),
            assistant: "Recent answer".to_string(),
        }];
        let compacted =
            build_compacted("---\ntest: true\n---\n\n", "\n", &kept, Path::new("archive.md"), 3);
        assert!(compacted.contains("3 earlier exchange(s) archived"));
        assert!(compacted.contains("## User\n\nRecent question"));
        assert!(compacted.contains("## Assistant\n\nRecent answer"));
        assert!(compacted.ends_with("## User\n\n"));
    }

    #[test]
    fn chrono_timestamp_format() {
        let ts = chrono_timestamp();
        // Should be YYYYMMDD-HHMMSS format
        assert_eq!(ts.len(), 15);
        assert_eq!(&ts[8..9], "-");
    }

    #[test]
    fn build_component_archive_format() {
        let doc = "---\nagent_doc_session: abc-123\nagent_doc_mode: stream\n---\n\n<!-- agent:exchange -->\nOld conversation\n<!-- /agent:exchange -->\n";
        let archive = build_component_archive(doc, "exchange", "\nOld conversation\n");
        assert!(archive.contains("archived_from: compact"));
        assert!(archive.contains("component: exchange"));
        assert!(archive.contains("session: abc-123"));
        assert!(archive.contains("Old conversation"));
    }
}
