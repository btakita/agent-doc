use agent_doc_response_toc::{LiveTocEntry, PromptFilters, live_section_window, live_sections};
use anyhow::{Context, Result};
use serde::Serialize;
use std::path::Path;

const DEFAULT_TOC_LIMIT: usize = 6;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum TocSource {
    Live,
    Archive,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TocEntry {
    locator: String,
    source: TocSource,
    heading: String,
    preview: String,
    refs: Vec<String>,
    archived_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct FetchSection {
    locator: String,
    source: TocSource,
    heading: String,
    refs: Vec<String>,
    archived_at: Option<String>,
    text: String,
}

pub fn run_toc(
    file: &Path,
    backlog_id: Option<&str>,
    query: Option<&str>,
    limit: usize,
    json: bool,
) -> Result<()> {
    let limit = limit.max(1);
    let entries = build_toc_entries(file, backlog_id, query, limit)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&entries)?);
        return Ok(());
    }

    if entries.is_empty() {
        println!("No response sections found.");
        return Ok(());
    }

    for entry in entries {
        let archived_at = entry
            .archived_at
            .as_deref()
            .map(|value| format!(" archived_at={value}"))
            .unwrap_or_default();
        let refs = if entry.refs.is_empty() {
            String::new()
        } else {
            format!(" refs={}", entry.refs.join(","))
        };
        println!(
            "{} [{}] {}{}{}",
            entry.locator,
            source_label(&entry.source),
            entry.heading,
            archived_at,
            refs
        );
        println!("  {}", entry.preview);
    }
    Ok(())
}

pub fn run_fetch(
    file: &Path,
    locator: &str,
    before: usize,
    after: usize,
    json: bool,
) -> Result<()> {
    let sections = fetch_sections(file, locator, before, after)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&sections)?);
        return Ok(());
    }

    for section in sections {
        let archived_at = section
            .archived_at
            .as_deref()
            .map(|value| format!(" archived_at={value}"))
            .unwrap_or_default();
        let refs = if section.refs.is_empty() {
            String::new()
        } else {
            format!(" refs={}", section.refs.join(","))
        };
        println!(
            "{} [{}] {}{}{}",
            section.locator,
            source_label(&section.source),
            section.heading,
            archived_at,
            refs
        );
        println!("{}", section.text);
        println!();
    }
    Ok(())
}

pub fn render_prompt_toc(file: &Path, doc: &str, prompt_targets: &[String]) -> Option<String> {
    let filters = PromptFilters::from_prompt_targets(prompt_targets);
    let live_entries = agent_doc_response_toc::live_toc_entries(
        doc,
        filters.backlog_ids.first().map(String::as_str),
        None,
        DEFAULT_TOC_LIMIT,
    )
    .unwrap_or_default()
    .into_iter()
    .map(live_entry_to_toc_entry)
    .collect::<Vec<_>>();
    let mut entries = live_entries;

    let archive_batches = if filters.backlog_ids.is_empty() {
        vec![archive_entries(file, None, None, DEFAULT_TOC_LIMIT / 2).unwrap_or_default()]
    } else {
        filters
            .backlog_ids
            .iter()
            .map(|backlog_id| {
                archive_entries(file, Some(backlog_id.as_str()), None, DEFAULT_TOC_LIMIT / 2)
                    .unwrap_or_default()
            })
            .collect()
    };

    for batch in archive_batches {
        for entry in batch {
            if !entries
                .iter()
                .any(|candidate| candidate.locator == entry.locator)
            {
                entries.push(entry);
            }
        }
    }

    if entries.is_empty() {
        return None;
    }

    let rendered = entries
        .into_iter()
        .map(|entry| {
            let archived_at = entry
                .archived_at
                .as_deref()
                .map(|value| format!(" archived_at=\"{}\"", value))
                .unwrap_or_default();
            let refs = if entry.refs.is_empty() {
                String::new()
            } else {
                format!("\nrefs: {}", entry.refs.join(", "))
            };
            format!(
                "<entry locator=\"{}\" source=\"{}\"{}>\nheading: {}\npreview: {}{}\n</entry>",
                entry.locator,
                source_label(&entry.source),
                archived_at,
                entry.heading,
                entry.preview,
                refs
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Some(rendered)
}

fn build_toc_entries(
    file: &Path,
    backlog_id: Option<&str>,
    query: Option<&str>,
    limit: usize,
) -> Result<Vec<TocEntry>> {
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let mut entries = agent_doc_response_toc::live_toc_entries(&content, backlog_id, query, limit)
        .unwrap_or_default()
        .into_iter()
        .map(live_entry_to_toc_entry)
        .collect::<Vec<_>>();
    for entry in archive_entries(file, backlog_id, query, limit)? {
        if !entries
            .iter()
            .any(|candidate| candidate.locator == entry.locator)
        {
            entries.push(entry);
        }
    }
    Ok(entries)
}

fn archive_entries(
    file: &Path,
    backlog_id: Option<&str>,
    query: Option<&str>,
    limit: usize,
) -> Result<Vec<TocEntry>> {
    let turns = agent_doc_sqlite::archive_index::list_recent_turns(file, query, backlog_id, limit)?;
    Ok(turns
        .into_iter()
        .map(|turn| TocEntry {
            locator: format!("archive:{}#{}", turn.archive_path, turn.turn_ordinal),
            source: TocSource::Archive,
            heading: turn.heading,
            preview: turn.preview,
            refs: turn.refs,
            archived_at: Some(turn.archived_at),
        })
        .collect())
}

fn fetch_sections(
    file: &Path,
    locator: &str,
    before: usize,
    after: usize,
) -> Result<Vec<FetchSection>> {
    if let Some(live) = locator.strip_prefix("live:") {
        let ordinal: usize = live
            .parse()
            .with_context(|| format!("invalid live locator '{}'", locator))?;
        let content = std::fs::read_to_string(file)
            .with_context(|| format!("failed to read {}", file.display()))?;
        let sections = live_sections(&content)?;
        return Ok(live_section_window(&sections, ordinal, before, after)?
            .iter()
            .map(|section| FetchSection {
                locator: format!("live:{}", section.ordinal),
                source: TocSource::Live,
                heading: section.heading.clone(),
                refs: section.refs.clone(),
                archived_at: None,
                text: section.text.clone(),
            })
            .collect());
    }

    let Some(archive_locator) = locator.strip_prefix("archive:") else {
        anyhow::bail!("unsupported locator '{}'", locator);
    };
    let Some((archive_path, ordinal_text)) = archive_locator.rsplit_once('#') else {
        anyhow::bail!("archive locator must look like archive:<path>#<turn>");
    };
    let turn_ordinal: i64 = ordinal_text
        .parse()
        .with_context(|| format!("invalid archive turn '{}'", ordinal_text))?;
    let turns = agent_doc_sqlite::archive_index::fetch_turn_window(
        file,
        archive_path,
        turn_ordinal,
        before,
        after,
    )?;
    Ok(turns
        .into_iter()
        .map(|turn| FetchSection {
            locator: format!("archive:{}#{}", turn.archive_path, turn.turn_ordinal),
            source: TocSource::Archive,
            heading: turn.heading,
            refs: turn.refs,
            archived_at: Some(turn.archived_at),
            text: turn.text,
        })
        .collect())
}

fn source_label(source: &TocSource) -> &'static str {
    match source {
        TocSource::Live => "live",
        TocSource::Archive => "archive",
    }
}

fn live_entry_to_toc_entry(entry: LiveTocEntry) -> TocEntry {
    TocEntry {
        locator: entry.locator,
        source: TocSource::Live,
        heading: entry.heading,
        preview: entry.preview,
        refs: entry.refs,
        archived_at: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetch_live_sections_returns_window() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: first — gpt-5\n\nBody one.\n\n",
            "### Re: second — gpt-5\n\nBody two.\n",
            "<!-- /agent:exchange -->\n",
        );
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), doc).unwrap();
        let sections = fetch_sections(tmp.path(), "live:2", 1, 0).unwrap();
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].heading, "### Re: first — gpt-5");
        assert_eq!(sections[1].heading, "### Re: second — gpt-5");
    }
}
