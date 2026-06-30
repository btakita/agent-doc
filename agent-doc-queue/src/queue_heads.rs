//! Active queue-head projection from document text.
//!
//! This module owns pure `agent:queue` head classification. Callers provide
//! document text; file IO, cycle-state persistence, and closeout guards stay in
//! orchestration.

/// Extract queue prompt head texts from a document's `agent:queue` component.
pub fn active_queue_heads(doc: &str) -> Vec<String> {
    queue_prompt_heads(doc)
}

/// Extract free-text (non-id-backed) queue prompt head texts from a document's
/// `agent:queue` component.
pub fn active_free_text_queue_heads(doc: &str) -> Vec<String> {
    queue_prompt_heads(doc)
        .into_iter()
        .filter(|text| !is_do_directive(text))
        .collect()
}

/// True when a queue head is id-backed: explicit `do [#id]` / `do #id`, or the
/// optional-`do` bare leading `[#id]` / `#id` form.
pub fn is_do_directive(text: &str) -> bool {
    let lower = text.trim().to_ascii_lowercase();
    lower.starts_with("do [#") || lower.starts_with("do #") || leads_with_bare_id_directive(&lower)
}

fn queue_prompt_heads(doc: &str) -> Vec<String> {
    let Ok(components) = agent_doc_element::element::parse(doc) else {
        return Vec::new();
    };
    let Some(queue) = components.iter().find(|c| c.name == "queue") else {
        return Vec::new();
    };
    let Ok(entries) = crate::document_queue::parse(queue.content(doc)) else {
        return Vec::new();
    };
    crate::document_queue::prompts(&entries)
        .into_iter()
        .map(|prompt| prompt.text.trim().to_string())
        .filter(|text| !text.is_empty())
        .collect()
}

/// Optional-`do` grammar: a queue head that leads with a bare id token (`[#id]`
/// or `#id`) is id-backed. A trailing `:` (`[#id]: note`) keeps the line inert
/// as prose annotation rather than a directive.
fn leads_with_bare_id_directive(lower: &str) -> bool {
    let (rest, bracketed) = if let Some(r) = lower.strip_prefix("[#") {
        (r, true)
    } else if let Some(r) = lower.strip_prefix('#') {
        (r, false)
    } else {
        return false;
    };
    let id_len = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .count();
    if id_len == 0 {
        return false;
    }
    let after = &rest[id_len..];
    if bracketed {
        match after.strip_prefix(']') {
            Some(tail) => !tail.starts_with(':'),
            None => false,
        }
    } else {
        after.is_empty() || after.starts_with([' ', '\t', '.'])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_do_directive_accepts_do_and_bare_id_forms() {
        // Back-compat: explicit `do` forms.
        assert!(is_do_directive("do [#opt]"));
        assert!(is_do_directive("do #opt"));
        assert!(is_do_directive("DO [#opt]. trailing note"));
        // Optional-`do`: bare id token is id-backed.
        assert!(is_do_directive("[#opt]"));
        assert!(is_do_directive("[#opt]. do the small fix"));
        assert!(is_do_directive("#opt"));
        assert!(is_do_directive("#opt do the thing"));
        // Inert: prose annotation (`[#id]:`), references, plain prose, headings.
        assert!(!is_do_directive("[#opt]: just a note"));
        assert!(!is_do_directive("#opt: just a note"));
        assert!(!is_do_directive("re [#opt]"));
        assert!(!is_do_directive("see [#opt] for context"));
        assert!(!is_do_directive("# heading"));
        assert!(!is_do_directive("just a free-text prompt"));
    }

    #[test]
    fn active_queue_heads_split_id_backed_and_free_text_prompts() {
        let doc = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:queue -->\n",
            "- do [#build]\n",
            "- [#bare] do the small fix\n",
            "- [#note]: this is annotation prose\n",
            "- write a status summary\n",
            "<!-- /agent:queue -->\n"
        );

        assert_eq!(
            active_queue_heads(doc),
            vec![
                "do [#build]".to_string(),
                "[#bare] do the small fix".to_string(),
                "[#note]: this is annotation prose".to_string(),
                "write a status summary".to_string(),
            ]
        );
        assert_eq!(
            active_free_text_queue_heads(doc),
            vec![
                "[#note]: this is annotation prose".to_string(),
                "write a status summary".to_string(),
            ]
        );
    }

    #[test]
    fn active_queue_heads_tolerate_missing_or_malformed_queue() {
        assert!(active_queue_heads("plain document").is_empty());
        assert!(active_free_text_queue_heads("plain document").is_empty());
    }
}
