//! Exchange element descriptor.

use std::collections::HashMap;

use agent_doc_element::{
    Component, ElementAuthority, ElementCompositionRole, ElementDescriptor, ElementRealtimeModel,
    ElementSchedulingRole, ElementShape, ElementSource, ElementWritePolicy,
};

pub const DESCRIPTOR: ElementDescriptor = ElementDescriptor {
    name: "exchange",
    aliases: &[],
    source: ElementSource::BuiltIn,
    shape: ElementShape::Component,
    authority: ElementAuthority::SharedOperatorAuthoritative,
    write_policy: ElementWritePolicy::MergeOnly,
    scheduling_role: ElementSchedulingRole::None,
    realtime_model: ElementRealtimeModel::Exchange,
    composition_role: ElementCompositionRole::LocalOnly,
    realtime: true,
};

pub fn descriptor() -> ElementDescriptor {
    DESCRIPTOR
}

/// Extract the byte length of the exchange component's trimmed content.
/// Returns 0 if no exchange component is found or component parsing fails.
pub fn exchange_content_len(doc: &str) -> usize {
    exchange_content(doc)
        .map(|content| content.trim().len())
        .unwrap_or(0)
}

pub fn exchange_content(doc: &str) -> Option<&str> {
    exchange_component(doc).map(|component| component.content(doc))
}

pub fn exchange_component(doc: &str) -> Option<Component> {
    agent_doc_element::element::parse(doc)
        .ok()?
        .into_iter()
        .find(|component| component.name == "exchange")
}

pub fn normalized_prompt_text(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("<!--")
        || trimmed.starts_with("### Re:")
        || trimmed.starts_with("## Assistant")
        || trimmed.starts_with("## User")
        || is_markdown_heading_line(trimmed)
    {
        return None;
    }
    Some(
        trimmed
            .strip_prefix('❯')
            .unwrap_or(trimmed)
            .trim()
            .to_string(),
    )
}

pub fn is_markdown_heading_line(trimmed: &str) -> bool {
    let hashes = trimmed.bytes().take_while(|byte| *byte == b'#').count();
    (1..=6).contains(&hashes) && trimmed.as_bytes().get(hashes) == Some(&b' ')
}

pub fn normalized_prompt_counts(exchange: &str) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for line in exchange.lines() {
        if let Some(text) = normalized_prompt_text(line) {
            *counts.entry(text).or_default() += 1;
        }
    }
    counts
}

pub fn split_line_segment(segment: &str) -> (&str, &str) {
    segment
        .strip_suffix('\n')
        .map(|line| (line, "\n"))
        .unwrap_or((segment, ""))
}

pub fn is_code_fence_delimiter(trimmed: &str) -> bool {
    let Some(first) = trimmed.chars().next() else {
        return false;
    };
    if first != '`' && first != '~' {
        return false;
    }
    trimmed.chars().take_while(|ch| *ch == first).count() >= 3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exchange_content_len_reports_trimmed_exchange_body() {
        let doc = "<!-- agent:exchange -->\nHello world\n<!-- /agent:exchange -->\n";
        assert_eq!(exchange_content_len(doc), "Hello world".len());

        let empty = "<!-- agent:exchange -->\n\n<!-- /agent:exchange -->\n";
        assert_eq!(exchange_content_len(empty), 0);

        let no_exchange = "Just text.";
        assert_eq!(exchange_content_len(no_exchange), 0);
    }

    #[test]
    fn normalized_prompt_text_ignores_exchange_structure() {
        assert_eq!(
            normalized_prompt_text("❯ ship it").as_deref(),
            Some("ship it")
        );
        assert_eq!(
            normalized_prompt_text("ship it").as_deref(),
            Some("ship it")
        );
        assert_eq!(normalized_prompt_text("### Re: ship it"), None);
        assert_eq!(normalized_prompt_text("## User"), None);
        assert_eq!(normalized_prompt_text("## Heading"), None);
        assert_eq!(normalized_prompt_text("<!-- agent:boundary:x -->"), None);
    }

    #[test]
    fn normalized_prompt_counts_counts_equivalent_prefixed_lines() {
        let counts = normalized_prompt_counts("❯ ship it\nship it\n### Re: ship it\n");

        assert_eq!(counts.get("ship it").copied(), Some(2));
    }

    #[test]
    fn code_fence_delimiter_detects_common_fences() {
        assert!(is_code_fence_delimiter("```"));
        assert!(is_code_fence_delimiter("~~~rust"));
        assert!(!is_code_fence_delimiter("``"));
        assert!(!is_code_fence_delimiter("text"));
    }
}
