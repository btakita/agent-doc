//! Queue response heading and prompt identity matching.
//!
//! This module owns pure response-to-queue-head matching policy. Callers provide
//! already-read document/response text; file IO and lifecycle mutations stay in
//! orchestration.

pub fn queue_prompt_done_id(text: &str) -> Option<String> {
    let marker = text.find('#')?;
    let tail = &text[marker + 1..];
    let id = tail
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        .collect::<String>();
    if id.is_empty() {
        None
    } else {
        Some(id.to_ascii_lowercase())
    }
}

pub fn normalize_done_id(id: &str) -> String {
    id.trim()
        .trim_start_matches('[')
        .trim_start_matches('#')
        .trim_end_matches(']')
        .to_ascii_lowercase()
}

pub fn display_queue_prompt_text(text: &str) -> String {
    text.lines()
        .map(|line| {
            let line = line.trim().trim_start_matches('❯').trim();
            crate::document_queue::strip_priority_markers(line)
                .replace("[#", "#")
                .replace(']', "")
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn normalize_queue_prompt_text(text: &str) -> String {
    display_queue_prompt_text(text).to_ascii_lowercase()
}

pub fn queue_prompt_text_matches(prompt_change: &str, queue_head: &str) -> bool {
    normalize_queue_prompt_text(prompt_change) == normalize_queue_prompt_text(queue_head)
}

pub fn response_heading_topic(line: &str) -> Option<&str> {
    let trimmed = line.trim().trim_start_matches('❯').trim();
    let topic = trimmed.strip_prefix("### Re:")?.trim();
    Some(
        topic
            .split_once(" — ")
            .map(|(topic, _)| topic)
            .unwrap_or(topic)
            .trim(),
    )
}

pub fn response_topic_matches_queue_head(topic: &str, queue_head: &str) -> bool {
    // Used by the Codex Stop-hook auto-close path, which has no closeout CLI
    // flags to express completion explicitly. Two completion shapes count:
    // 1. An exact topic match (`### Re: do [#foo]` vs head `do [#foo]`).
    // 2. A topic that resolves to EXACTLY the head id (`### Re: #fix1` vs head
    //    `do #fix1`) -- the Codex auto-loop titles a clean completion with the
    //    head's `#id` (#queue-head-consume-on-topic-id-regression).
    if queue_prompt_text_matches(topic, queue_head) {
        return true;
    }
    queue_prompt_done_id(queue_head)
        .is_some_and(|head_id| crate::queue_directive::topic_resolves_to_exact_id(topic, &head_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_prompt_done_id_parses_canonical_forms() {
        assert_eq!(
            queue_prompt_done_id("do [#jbrsrbusyint]"),
            Some("jbrsrbusyint".to_string())
        );
        assert_eq!(
            queue_prompt_done_id("do #jbrsrbusyint more text"),
            Some("jbrsrbusyint".to_string())
        );
        assert_eq!(queue_prompt_done_id("plain prompt"), None);
    }

    #[test]
    fn normalize_done_id_strips_common_markers() {
        assert_eq!(normalize_done_id(" [#Fix-1] "), "fix-1");
        assert_eq!(normalize_done_id("#Fix_2"), "fix_2");
    }

    #[test]
    fn queue_prompt_text_match_ignores_cosmetic_prompt_markers() {
        assert!(queue_prompt_text_matches("do [#foo]", ":pushpin: do #foo"));
        assert!(queue_prompt_text_matches("❯ do [#Foo]", "do #foo"));
        assert!(!queue_prompt_text_matches("do [#foo]", "do [#bar]"));
    }

    #[test]
    fn response_heading_topic_extracts_canonical_topic() {
        assert_eq!(
            response_heading_topic("  ❯ ### Re: do [#foo] — follow-up"),
            Some("do [#foo]")
        );
        assert_eq!(response_heading_topic("### Re: #foo"), Some("#foo"));
        assert_eq!(response_heading_topic("## Re: #foo"), None);
    }

    #[test]
    fn heading_topic_matches_head_exactly_or_by_exact_id() {
        // Codex Stop-hook path: exact-topic match, or a topic that resolves to
        // EXACTLY the head id (#queue-head-consume-on-topic-id-regression).
        assert!(response_topic_matches_queue_head("do [#foo]", "do [#foo]"));
        assert!(response_topic_matches_queue_head(
            "do [#foo]",
            ":pushpin: do [#foo]"
        ));
        assert!(response_topic_matches_queue_head("#fix1", "do #fix1"));
        assert!(response_topic_matches_queue_head("#foo", "do [#foo]"));
        // Halt/modifier headings must NOT count as completion (#queue-strike-on-halt).
        assert!(!response_topic_matches_queue_head("#foo halt", "do [#foo]"));
        assert!(!response_topic_matches_queue_head(
            "#foo deferred",
            "do [#foo]"
        ));
    }
}
