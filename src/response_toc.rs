use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::Path;

const DEFAULT_TOC_LIMIT: usize = 6;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum TocSource {
    Live,
    Archive,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct TocEntry {
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

#[derive(Debug, Clone)]
struct LiveSection {
    ordinal: usize,
    heading: String,
    text: String,
    refs: Vec<String>,
    normalized_text: String,
}

pub(crate) fn run_toc(
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

pub(crate) fn run_fetch(
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

pub(crate) fn render_prompt_toc(
    file: &Path,
    doc: &str,
    prompt_targets: &[String],
) -> Option<String> {
    let filters = PromptFilters::from_prompt_targets(prompt_targets);
    let live_entries = live_entries(
        doc,
        filters.backlog_ids.first().map(String::as_str),
        None,
        DEFAULT_TOC_LIMIT,
    )
    .unwrap_or_default();
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
    let mut entries = live_entries(&content, backlog_id, query, limit).unwrap_or_default();
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
    let turns = crate::archive_index::list_recent_turns(file, query, backlog_id, limit)?;
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
        let target_idx = ordinal
            .checked_sub(1)
            .filter(|idx| *idx < sections.len())
            .with_context(|| format!("live response {} not found", ordinal))?;
        let start = target_idx.saturating_sub(before);
        let end = (target_idx + after + 1).min(sections.len());
        return Ok(sections[start..end]
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
    let turns =
        crate::archive_index::fetch_turn_window(file, archive_path, turn_ordinal, before, after)?;
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

fn live_entries(
    doc: &str,
    backlog_id: Option<&str>,
    query: Option<&str>,
    limit: usize,
) -> Result<Vec<TocEntry>> {
    let sections = live_sections(doc)?;
    let backlog_id = backlog_id.map(normalize_backlog_id).transpose()?;
    let query_norm = query.map(normalize_text);
    let matching = sections
        .into_iter()
        .filter(|section| {
            backlog_id
                .as_deref()
                .map(|needle| section.refs.iter().any(|candidate| candidate == needle))
                .unwrap_or(true)
                && query_norm
                    .as_deref()
                    .map(|needle| section.normalized_text.contains(needle))
                    .unwrap_or(true)
        })
        .collect::<Vec<_>>();

    let keep_from = matching.len().saturating_sub(limit);
    Ok(matching[keep_from..]
        .iter()
        .map(|section| TocEntry {
            locator: format!("live:{}", section.ordinal),
            source: TocSource::Live,
            heading: section.heading.clone(),
            preview: preview_text(&section.text),
            refs: section.refs.clone(),
            archived_at: None,
        })
        .collect())
}

fn live_sections(doc: &str) -> Result<Vec<LiveSection>> {
    let components = crate::component::parse(doc)
        .with_context(|| "failed to parse document components for response TOC")?;
    let exchange = components
        .iter()
        .find(|component| component.name == "exchange")
        .with_context(|| "exchange component not found")?;
    Ok(collect_live_sections(exchange.content(doc)))
}

fn collect_live_sections(exchange_body: &str) -> Vec<LiveSection> {
    let mut sections = Vec::new();
    let mut current = Vec::new();

    for line in exchange_body.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("<!-- agent:boundary:") {
            break;
        }
        if trimmed.starts_with("### Re:") && !current.is_empty() {
            push_live_section(&mut sections, &mut current);
        }
        if !current.is_empty() || trimmed.starts_with("### Re:") {
            current.push(line.to_string());
        }
    }
    if !current.is_empty() {
        push_live_section(&mut sections, &mut current);
    }
    sections
}

fn push_live_section(sections: &mut Vec<LiveSection>, current: &mut Vec<String>) {
    let text = current.join("\n").trim().to_string();
    current.clear();
    if text.is_empty() {
        return;
    }
    let heading = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("### Re:")
        .trim()
        .to_string();
    let refs = extract_backlog_ids(&text).into_iter().collect::<Vec<_>>();
    sections.push(LiveSection {
        ordinal: sections.len() + 1,
        heading,
        normalized_text: normalize_text(&text),
        refs,
        text,
    });
}

fn extract_backlog_ids(text: &str) -> BTreeSet<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut found = BTreeSet::new();
    let mut idx = 0;
    while idx < chars.len() {
        if chars[idx] == '#'
            && (idx == 0 || chars[idx - 1] != '#')
            && idx + 1 < chars.len()
            && is_backlog_id_char(chars[idx + 1])
        {
            let mut end = idx + 1;
            while end < chars.len() && is_backlog_id_char(chars[end]) {
                end += 1;
            }
            found.insert(chars[idx..end].iter().collect());
            idx = end;
            continue;
        }
        idx += 1;
    }
    found
}

fn preview_text(text: &str) -> String {
    let collapsed = normalize_text(text);
    let mut chars = collapsed.chars();
    let preview: String = chars.by_ref().take(120).collect();
    if chars.next().is_some() {
        format!("{}...", preview.chars().take(117).collect::<String>())
    } else {
        preview
    }
}

fn normalize_text(text: &str) -> String {
    text.split_whitespace()
        .map(|segment| segment.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_backlog_id(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        anyhow::bail!("backlog id cannot be empty");
    }
    Ok(if trimmed.starts_with('#') {
        trimmed.to_string()
    } else {
        format!("#{trimmed}")
    })
}

fn is_backlog_id_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'
}

struct PromptFilters {
    backlog_ids: Vec<String>,
}

impl PromptFilters {
    fn from_prompt_targets(prompt_targets: &[String]) -> Self {
        let mut backlog_ids = BTreeSet::new();
        for target in prompt_targets {
            backlog_ids.extend(extract_backlog_ids(target));
        }
        Self {
            backlog_ids: backlog_ids.into_iter().collect(),
        }
    }
}

fn source_label(source: &TocSource) -> &'static str {
    match source {
        TocSource::Live => "live",
        TocSource::Archive => "archive",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_live_sections_skips_summary_and_boundary_tail() {
        let exchange = concat!(
            "### Session Summary\n\nCompacted.\n\n",
            "### Re: first — gpt-5\n\nBody one.\n\n",
            "### Re: second — gpt-5\n\nBody #restoc.\n",
            "<!-- agent:boundary:test -->\n",
            "do [#restoc]. spec-test-build-install-commit-push\n",
        );
        let sections = collect_live_sections(exchange);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].heading, "### Re: first — gpt-5");
        assert_eq!(sections[1].refs, vec!["#restoc".to_string()]);
    }

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
