//! Pure archive and summary rendering helpers for compact operations.

use agent_doc_element::element;
use agent_doc_frontmatter::frontmatter;
use agent_doc_topic::summarize_compacted_exchange;

use crate::compact_projection::CompactExchange;

/// Metadata rendered into compact archive frontmatter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactArchiveMetadata {
    pub archived_at: String,
    pub component: String,
    pub exchange_count: Option<usize>,
    pub document: Option<String>,
    pub session: Option<String>,
}

impl CompactArchiveMetadata {
    pub fn component(archived_at: impl Into<String>, component: impl Into<String>) -> Self {
        Self {
            archived_at: archived_at.into(),
            component: component.into(),
            exchange_count: None,
            document: None,
            session: None,
        }
    }

    pub fn with_exchange_count(mut self, exchange_count: usize) -> Self {
        self.exchange_count = Some(exchange_count);
        self
    }

    pub fn with_document(mut self, document: Option<String>) -> Self {
        self.document = document;
        self
    }

    pub fn with_session(mut self, session: Option<String>) -> Self {
        self.session = session;
        self
    }
}

/// Extract the session value that compact archives preserve in frontmatter.
pub fn compact_archive_session(original: &str) -> Option<String> {
    frontmatter::parse(original)
        .ok()
        .and_then(|(fm, _)| fm.session.clone())
}

/// Build archive content from a component body.
pub fn build_component_archive_content(metadata: &CompactArchiveMetadata, content: &str) -> String {
    let mut archive = render_archive_frontmatter(metadata);
    archive.push_str(content.trim());
    archive.push('\n');
    archive
}

/// Build archive content from inline-mode exchange pairs.
pub fn build_inline_exchange_archive_content(
    metadata: &CompactArchiveMetadata,
    exchanges: &[CompactExchange],
) -> String {
    let mut archive = render_archive_frontmatter(metadata);

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

/// Build the default visible summary for a full exchange compact.
pub fn build_exchange_compact_summary(content: &str, archive_path: &str) -> String {
    let mut summary = String::from("### Session Summary\n\n");
    summary.push_str(&format!(
        "*Compacted. Content archived to `{}`*\n",
        archive_path
    ));

    let Ok(components) = element::parse(content) else {
        return summary;
    };

    if let Some(exchange) = components.iter().find(|c| c.name == "exchange") {
        let digest = summarize_compacted_exchange(exchange.content(content));
        append_compact_summary_section(&mut summary, "Compacted content", &digest);
    }

    summary
}

/// Extract compact archive pointers from visible compact summary text.
pub fn compact_archive_pointers(content: &str) -> Vec<&str> {
    content
        .split("archived to `")
        .skip(1)
        .filter_map(|tail| tail.split_once('`').map(|(path, _)| path.trim()))
        .filter(|path| !path.is_empty())
        .collect()
}

/// Format a UTC archive timestamp as `YYYYMMDD-HHMMSS`.
pub fn format_compact_timestamp_from_unix_secs(secs: u64) -> String {
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    let mut year = 1970i64;
    let mut remaining_days = days as i64;
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        year += 1;
    }

    let month_days: &[i64] = if is_leap_year(year) {
        &[31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        &[31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month = 0;
    for &days_in_month in month_days {
        if remaining_days < days_in_month {
            break;
        }
        remaining_days -= days_in_month;
        month += 1;
    }

    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        year,
        month + 1,
        remaining_days + 1,
        hours,
        minutes,
        seconds
    )
}

fn render_archive_frontmatter(metadata: &CompactArchiveMetadata) -> String {
    let mut archive = String::new();
    archive.push_str("---\n");
    archive.push_str("archived_from: compact\n");
    archive.push_str(&format!("archived_at: {}\n", metadata.archived_at));
    archive.push_str(&format!("component: {}\n", metadata.component));
    if let Some(exchange_count) = metadata.exchange_count {
        archive.push_str(&format!("exchange_count: {}\n", exchange_count));
    }
    if let Some(document) = &metadata.document {
        archive.push_str(&format!("document: {}\n", document));
    }
    if let Some(session) = &metadata.session {
        archive.push_str(&format!("session: {}\n", session));
    }
    archive.push_str("---\n\n");
    archive
}

fn append_compact_summary_section(summary: &mut String, title: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    summary.push('\n');
    summary.push_str(title);
    summary.push_str(":\n");
    for item in items {
        summary.push_str("- ");
        summary.push_str(item);
        summary.push('\n');
    }
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(component: &str) -> CompactArchiveMetadata {
        CompactArchiveMetadata::component("20260701-123456", component)
            .with_document(Some("docs/session.md".to_string()))
            .with_session(Some("abc-123".to_string()))
    }

    #[test]
    fn build_inline_exchange_archive_renders_frontmatter_and_pairs() {
        let exchanges = vec![CompactExchange {
            user: "Hello".to_string(),
            assistant: "Hi there".to_string(),
        }];

        let archive = build_inline_exchange_archive_content(
            &metadata("exchange").with_exchange_count(exchanges.len()),
            &exchanges,
        );

        assert!(archive.contains("archived_from: compact"));
        assert!(archive.contains("archived_at: 20260701-123456"));
        assert!(archive.contains("component: exchange"));
        assert!(archive.contains("exchange_count: 1"));
        assert!(archive.contains("document: docs/session.md"));
        assert!(archive.contains("session: abc-123"));
        assert!(archive.contains("## User\n\nHello"));
        assert!(archive.contains("## Assistant\n\nHi there"));
    }

    #[test]
    fn build_component_archive_renders_trimmed_content() {
        let archive =
            build_component_archive_content(&metadata("exchange"), "\nOld conversation\n");

        assert!(archive.contains("component: exchange"));
        assert!(archive.contains("document: docs/session.md"));
        assert!(archive.contains("session: abc-123"));
        assert!(archive.ends_with("Old conversation\n"));
    }

    #[test]
    fn compact_archive_session_reads_supported_frontmatter_names() {
        let session = compact_archive_session(
            "---\nagent_doc_session: test-session\nagent_doc_format: template\n---\n\nbody",
        );
        assert_eq!(session.as_deref(), Some("test-session"));
    }

    #[test]
    fn compact_archive_pointers_extracts_non_empty_archive_paths() {
        let content = concat!(
            "*Compacted. Content archived to `.agent-doc/archives/a.md`*\n",
            "*2 earlier exchange(s) archived to ` .agent-doc/archives/b.md `*\n",
            "*Compacted. Content archived to ``*\n"
        );

        assert_eq!(
            compact_archive_pointers(content),
            vec![".agent-doc/archives/a.md", ".agent-doc/archives/b.md"]
        );
    }

    #[test]
    fn exchange_compact_summary_includes_archived_topic_digest() {
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: topic one\n\nResponse one.\n\n",
            "### Re: topic two\n\nResponse two.\n",
            "<!-- /agent:exchange -->\n"
        );

        let summary = build_exchange_compact_summary(content, ".agent-doc/archives/a.md");

        assert!(summary.contains("### Session Summary"));
        assert!(summary.contains("*Compacted. Content archived to `.agent-doc/archives/a.md`*"));
        assert!(summary.contains("Compacted content:"));
        assert!(summary.contains("Archived 2 response topic(s): topic one; topic two"));
    }

    #[test]
    fn compact_timestamp_formats_utc_leap_day() {
        // 2024-02-29 23:59:59 UTC.
        assert_eq!(
            format_compact_timestamp_from_unix_secs(1_709_251_199),
            "20240229-235959"
        );
    }
}
