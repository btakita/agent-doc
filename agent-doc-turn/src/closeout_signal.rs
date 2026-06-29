//! Pure closeout response and done-signal classification.
//!
//! Session-check owns file/cycle-state IO. This module owns the string-level
//! turn policy that decides whether response text completes a tracked-work id
//! and whether prompt text carries explicit done/resolved signals.

/// True when response text clearly completes `id`.
///
/// Completion is signalled by a response HEADING whose topic RESOLVES to exactly
/// this id, never by a bare prose citation of `#id` in the body
/// (#pending-done-guard-false-positive). A heading match plus a completion
/// marker still distinguishes a real completion from a halt/refusal response
/// that merely names the head (#queue-strike-on-halt).
pub fn response_clearly_completes_pending_id(response_text: &str, id: &str) -> bool {
    if !response_heading_resolves_to_pending_id(response_text, id) {
        return false;
    }
    contains_completion_marker(&response_text.to_ascii_lowercase())
}

/// True when some `### Re:` response heading's topic resolves to `#id`.
///
/// A batch `do [#a] [#b] ...` directive heading resolves to every bracketed id;
/// a titled `#id descriptive text` heading resolves only to its LEADING id. A
/// heading that merely contains `#id` later in descriptive prose, and any `#id`
/// cited in the response body, never resolves to it.
pub fn response_heading_resolves_to_pending_id(response_text: &str, id: &str) -> bool {
    let id_lower = id.to_ascii_lowercase();
    for raw in response_text.lines() {
        let line = raw.trim().to_ascii_lowercase();
        let Some(after) = line.strip_prefix('#') else {
            continue;
        };
        let heading = after.trim_start_matches('#').trim_start();
        let Some(topic) = heading.strip_prefix("re:") else {
            continue;
        };
        let topic = topic.split(" — ").next().unwrap_or(topic).trim();
        if let Some(do_list) = topic.strip_prefix("do ") {
            let bracket_ids = extract_bracket_ids(do_list);
            if !bracket_ids.is_empty() {
                if bracket_ids.iter().any(|b| b == &id_lower) {
                    return true;
                }
                continue;
            }
            if leading_hash_id(do_list).as_deref() == Some(id_lower.as_str()) {
                return true;
            }
        } else if leading_hash_id(topic).as_deref() == Some(id_lower.as_str()) {
            return true;
        }
    }
    false
}

fn leading_hash_id(text: &str) -> Option<String> {
    let t = text.strip_prefix('[').unwrap_or(text);
    let rest = t.strip_prefix('#')?;
    let id: String = rest
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        .collect();
    (!id.is_empty()).then_some(id)
}

fn extract_bracket_ids(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(pos) = rest.find("[#") {
        let after = &rest[pos + 2..];
        let id: String = after
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
            .collect();
        let consumed = id.len();
        if !id.is_empty() {
            out.push(id);
        }
        rest = &after[consumed..];
    }
    out
}

fn contains_completion_marker(text: &str) -> bool {
    [
        "implemented",
        "fixed",
        "done.",
        "done ",
        "completed",
        "updated",
        "verification:",
        "verified",
        "pushed",
        "commit:",
        "outcome:",
        "what changed:",
        "landed",
        "shipped",
    ]
    .iter()
    .any(|marker| text.contains(marker))
}

pub fn explicit_done_signal_ids(text: &str) -> Vec<String> {
    let normalized = normalize_done_signal_text(text);
    if normalized.is_empty() {
        return Vec::new();
    }

    let lower = normalized.to_ascii_lowercase();
    let is_done_signal = lower.contains(" done")
        || lower.ends_with(" done")
        || lower.starts_with("done ")
        || lower.contains(" complete")
        || lower.ends_with(" complete")
        || lower.starts_with("complete ")
        || lower.contains(" completed")
        || lower.ends_with(" completed")
        || lower.starts_with("completed ")
        || lower.contains(" resolved")
        || lower.ends_with(" resolved")
        || lower.starts_with("resolved ");
    if !is_done_signal {
        return Vec::new();
    }

    agent_doc_element_backlog::backlog::extract_pending_hash_ids(&normalized)
}

pub fn plain_done_signal(text: &str) -> bool {
    let normalized = normalize_done_signal_text(text);
    let lower = normalized.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "done"
            | "done."
            | "complete"
            | "complete."
            | "completed"
            | "completed."
            | "resolved"
            | "resolved."
    )
}

fn normalize_done_signal_text(text: &str) -> String {
    text.trim()
        .trim_start_matches('❯')
        .trim()
        .trim_start_matches("- ")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_completion_requires_matching_heading_and_completion_marker() {
        let response = "### Re: do [#alpha] — gpt-5\n\nImplemented the fix.\n\nRelated to #beta.";
        assert!(response_clearly_completes_pending_id(response, "alpha"));
        assert!(!response_clearly_completes_pending_id(response, "beta"));
        assert!(!response_clearly_completes_pending_id(
            "### Re: do [#alpha]\n\nBlocked on CI.",
            "alpha"
        ));
    }

    #[test]
    fn response_heading_resolution_uses_heading_topic_not_body_mentions() {
        assert!(response_heading_resolves_to_pending_id(
            "### Re: do [#a] [#b]\n\nDone.",
            "b"
        ));
        assert!(response_heading_resolves_to_pending_id(
            "### Re: #lead descriptive text\n\nDone.",
            "lead"
        ));
        assert!(!response_heading_resolves_to_pending_id(
            "### Re: unrelated #body\n\nDone #body.",
            "body"
        ));
    }

    #[test]
    fn done_signals_extract_explicit_ids_and_plain_single_review_signals() {
        assert_eq!(explicit_done_signal_ids("❯ #abc done"), vec!["abc"]);
        assert_eq!(
            explicit_done_signal_ids("- complete #abc and #def"),
            vec!["abc", "def"]
        );
        assert!(plain_done_signal("❯ done."));
        assert!(!plain_done_signal("done #abc"));
        assert!(explicit_done_signal_ids("look at #abc").is_empty());
    }
}
