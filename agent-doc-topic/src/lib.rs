//! # Module: topic
//!
//! ## Spec
//! - Pure text parsing of `### Re:` topic sections from an exchange component body.
//! - Splits a component body into preamble (before the first `### Re:` heading), the per-topic
//!   sections, and any trailing content after a managed `<!-- agent:boundary: -->` marker.
//! - Boundary markers are managed by the binary and are never archived, so they are stripped
//!   from the parsed output.
//!
//! ## Agentic Contracts
//! - Lives in `agent-doc-topic` so compaction/archive flows can parse topic sections without
//!   depending on the `agent-doc-core` compatibility facade.
//!
//! ## Evals
//! - parse_topic_sections_basic: `### Re:` headings split into sections
//! - parse_topic_sections_strips_boundary_marker: boundary markers excluded from sections

/// Parsed topic-section view of an exchange component body.
#[derive(Debug, Default)]
pub struct TopicSections {
    /// Content before the first `### Re:` heading.
    pub preamble: String,
    /// One entry per `### Re:` topic heading, including the heading line.
    pub sections: Vec<String>,
    /// Content after a managed `<!-- agent:boundary: -->` marker.
    pub trailing: String,
}

/// Split a component body into its preamble and per-topic sections.
pub fn parse_topic_sections(content: &str) -> (String, Vec<String>) {
    let parsed = parse_topic_sections_with_tail(content);
    (parsed.preamble, parsed.sections)
}

/// Split a component body into preamble, per-topic sections, and post-boundary trailing content.
pub fn parse_topic_sections_with_tail(content: &str) -> TopicSections {
    let mut preamble = String::new();
    let mut sections: Vec<String> = Vec::new();
    let mut current: Option<String> = None;
    let mut found_first = false;
    let mut after_boundary = false;
    let mut trailing = String::new();

    for line in content.lines() {
        // Strip boundary markers — they are managed by the binary, not archived
        if line.starts_with("<!-- agent:boundary:") {
            after_boundary = true;
            continue;
        }
        if after_boundary {
            trailing.push_str(line);
            trailing.push('\n');
            continue;
        }

        if line.starts_with("### Re:") || line.starts_with("#### Re:") || line.starts_with("## Re:")
        {
            if let Some(prev) = current.take() {
                sections.push(prev);
            }
            found_first = true;
            current = Some(format!("{}\n", line));
        } else if found_first {
            let section = current.get_or_insert_with(String::new);
            section.push_str(line);
            section.push('\n');
        } else {
            preamble.push_str(line);
            preamble.push('\n');
        }
    }

    if let Some(last) = current {
        sections.push(last);
    }

    TopicSections {
        preamble,
        sections,
        trailing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_topic_sections_basic() {
        let content = "intro\n### Re: a\nbody a\n### Re: b\nbody b\n";
        let (preamble, sections) = parse_topic_sections(content);
        assert_eq!(preamble.trim(), "intro");
        assert_eq!(sections.len(), 2);
        assert!(sections[0].starts_with("### Re: a"));
        assert!(sections[1].starts_with("### Re: b"));
    }

    #[test]
    fn parse_topic_sections_strips_boundary_marker() {
        let content = "### Re: a\nbody a\n<!-- agent:boundary:abcd1234 -->\ntail\n";
        let parsed = parse_topic_sections_with_tail(content);
        assert_eq!(parsed.sections.len(), 1);
        assert!(!parsed.sections[0].contains("boundary"));
        assert_eq!(parsed.trailing.trim(), "tail");
    }

    #[test]
    fn parse_topic_sections_no_re_headings() {
        let content = "just preamble\nmore preamble\n";
        let (preamble, sections) = parse_topic_sections(content);
        assert!(sections.is_empty());
        assert_eq!(preamble.trim(), "just preamble\nmore preamble");
    }
}
