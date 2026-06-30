use anyhow::{Context, Result};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveTocEntry {
    pub locator: String,
    pub heading: String,
    pub preview: String,
    pub refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveSection {
    pub ordinal: usize,
    pub heading: String,
    pub text: String,
    pub refs: Vec<String>,
    normalized_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptFilters {
    pub backlog_ids: Vec<String>,
}

impl PromptFilters {
    pub fn from_prompt_targets(prompt_targets: &[String]) -> Self {
        let mut backlog_ids = BTreeSet::new();
        for target in prompt_targets {
            backlog_ids.extend(extract_backlog_ids(target));
        }
        Self {
            backlog_ids: backlog_ids.into_iter().collect(),
        }
    }
}

pub fn live_toc_entries(
    doc: &str,
    backlog_id: Option<&str>,
    query: Option<&str>,
    limit: usize,
) -> Result<Vec<LiveTocEntry>> {
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
        .map(|section| LiveTocEntry {
            locator: format!("live:{}", section.ordinal),
            heading: section.heading.clone(),
            preview: preview_text(&section.text),
            refs: section.refs.clone(),
        })
        .collect())
}

pub fn live_sections(doc: &str) -> Result<Vec<LiveSection>> {
    let components = agent_doc_element::element::parse(doc)
        .with_context(|| "failed to parse document components for response TOC")?;
    let exchange = components
        .iter()
        .find(|component| component.name == "exchange")
        .with_context(|| "exchange component not found")?;
    Ok(collect_live_sections(exchange.content(doc)))
}

pub fn collect_live_sections(exchange_body: &str) -> Vec<LiveSection> {
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

pub fn live_section_window(
    sections: &[LiveSection],
    ordinal: usize,
    before: usize,
    after: usize,
) -> Result<&[LiveSection]> {
    let target_idx = ordinal
        .checked_sub(1)
        .filter(|idx| *idx < sections.len())
        .with_context(|| format!("live response {} not found", ordinal))?;
    let start = target_idx.saturating_sub(before);
    let end = (target_idx + after + 1).min(sections.len());
    Ok(&sections[start..end])
}

pub fn extract_backlog_ids(text: &str) -> BTreeSet<String> {
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

pub fn preview_text(text: &str) -> String {
    let collapsed = normalize_text(text);
    let mut chars = collapsed.chars();
    let preview: String = chars.by_ref().take(120).collect();
    if chars.next().is_some() {
        format!("{}...", preview.chars().take(117).collect::<String>())
    } else {
        preview
    }
}

pub fn normalize_text(text: &str) -> String {
    text.split_whitespace()
        .map(|segment| segment.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn normalize_backlog_id(raw: &str) -> Result<String> {
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

fn is_backlog_id_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_with_exchange(exchange_body: &str) -> String {
        format!(
            concat!(
                "---\nagent_doc_format: template\n---\n\n",
                "## Exchange\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "{}",
                "\n<!-- /agent:exchange -->\n",
            ),
            exchange_body
        )
    }

    #[test]
    fn collect_live_sections_skips_summary_and_boundary_tail() {
        let exchange = concat!(
            "### Session Summary\n\nCompacted.\n\n",
            "### Re: first - gpt-5\n\nBody one.\n\n",
            "### Re: second - gpt-5\n\nBody #restoc.\n",
            "<!-- agent:boundary:test -->\n",
            "do [#restoc]. spec-test-build-install-commit-push\n",
        );
        let sections = collect_live_sections(exchange);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].heading, "### Re: first - gpt-5");
        assert_eq!(sections[1].refs, vec!["#restoc".to_string()]);
    }

    #[test]
    fn live_toc_entries_filters_query_and_keeps_last_matches() {
        let doc = doc_with_exchange(concat!(
            "### Re: first - gpt-5\n\nNeedle alpha #work.\n\n",
            "### Re: second - gpt-5\n\nNo match #work.\n\n",
            "### Re: third - gpt-5\n\nNeedle beta #work.\n",
        ));
        let entries = live_toc_entries(&doc, Some("work"), Some("NEEDLE"), 1).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].locator, "live:3");
        assert_eq!(entries[0].refs, vec!["#work".to_string()]);
    }

    #[test]
    fn live_section_window_returns_requested_context() {
        let sections = collect_live_sections(concat!(
            "### Re: first - gpt-5\n\nBody one.\n\n",
            "### Re: second - gpt-5\n\nBody two.\n\n",
            "### Re: third - gpt-5\n\nBody three.\n",
        ));
        let window = live_section_window(&sections, 2, 1, 1).unwrap();
        assert_eq!(window.len(), 3);
        assert_eq!(window[0].heading, "### Re: first - gpt-5");
        assert_eq!(window[2].heading, "### Re: third - gpt-5");
    }

    #[test]
    fn backlog_ids_are_extracted_and_normalized() {
        let ids = extract_backlog_ids("## Heading #one #two-2 ### ignored #one");
        assert_eq!(
            ids.into_iter().collect::<Vec<_>>(),
            vec!["#one".to_string(), "#two-2".to_string()]
        );
        assert_eq!(normalize_backlog_id("one").unwrap(), "#one");
        assert_eq!(normalize_backlog_id("#one").unwrap(), "#one");
        assert!(normalize_backlog_id("  ").is_err());
    }

    #[test]
    fn prompt_filters_collect_backlog_ids() {
        let filters = PromptFilters::from_prompt_targets(&[
            "review #two and #one".to_string(),
            "again #one".to_string(),
        ]);
        assert_eq!(
            filters.backlog_ids,
            vec!["#one".to_string(), "#two".to_string()]
        );
    }

    #[test]
    fn preview_text_collapses_case_and_truncates() {
        let preview = preview_text(&format!("{}\n{}", "Mixed CASE", "x".repeat(140)));
        assert!(preview.starts_with("mixed case "));
        assert_eq!(preview.len(), 120);
        assert!(preview.ends_with("..."));
    }
}
