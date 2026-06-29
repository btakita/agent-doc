//! Pure closeout response and done-signal classification.
//!
//! Session-check owns file/cycle-state IO. This module owns the string-level
//! turn policy that decides whether response text completes a tracked-work id
//! and whether prompt text carries explicit done/resolved signals.

/// Where a directive id's response heading materialized.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResponseSource {
    Exchange,
    Archive,
}

impl ResponseSource {
    pub fn as_str(self) -> &'static str {
        match self {
            ResponseSource::Exchange => "exchange",
            ResponseSource::Archive => "archive",
        }
    }
}

/// Resolve where `id`'s `### Re:` response heading materialized, if anywhere.
///
/// Pure over already-resolved `content` (the live committed exchange) and
/// `archives` (HEAD compact-archive bodies). The caller owns all file/archive
/// IO and passes the string bodies here.
pub fn directive_response_source(
    content: &str,
    archives: &[String],
    id: &str,
) -> Option<ResponseSource> {
    if content_has_re_heading_for_id(content, id) {
        return Some(ResponseSource::Exchange);
    }
    if archives
        .iter()
        .any(|archive| content_has_re_heading_for_id(archive, id))
    {
        return Some(ResponseSource::Archive);
    }
    None
}

/// True when any `### Re:` heading line in `content` references `#id` / `[#id]`.
///
/// `do #id` responses always render under a `### Re: ... #id` heading, so a
/// heading-scoped match avoids false matches against queue-prompt echoes or
/// backlog lines that merely mention the id.
pub fn content_has_re_heading_for_id(content: &str, id: &str) -> bool {
    if id.is_empty() {
        return false;
    }
    let needle = format!("#{}", id.to_ascii_lowercase());
    content.lines().any(|line| {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("### Re:") && !trimmed.starts_with("###Re") {
            return false;
        }
        let lower = trimmed.to_ascii_lowercase();
        match lower.find(&needle) {
            None => false,
            Some(pos) => {
                // Reject a longer-id prefix collision (`#ab` must not match `#abc`).
                let after = &lower[pos + needle.len()..];
                !after
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            }
        }
    })
}

/// Pure inputs for per-id queue directive response-loss detection.
///
/// All ids are normalized (no leading `#`, lowercased) before they reach this
/// policy. The caller resolves `content` and `archives` up front so this logic
/// stays deterministic and IO-free.
#[derive(Clone, Debug)]
pub struct ReapedResponseLossInput<'a> {
    /// `do #id` directive target ids active this cycle.
    pub directive_ids: &'a [String],
    /// Pending ids reaped into `agent:done` this cycle.
    pub reaped_ids: &'a [String],
    /// Live committed exchange content.
    pub content: &'a str,
    /// HEAD-referenced compact-archive bodies (each searched like `content`).
    pub archives: &'a [String],
}

/// Reaped `do #id` directive ids whose `### Re: ... #id` response did not land.
///
/// Returns the directive ids whose response heading did not materialize in the
/// live exchange `content` or in any HEAD compact `archives` entry. Order follows
/// `directive_ids`; duplicates are collapsed.
pub fn reaped_directive_ids_without_response(input: &ReapedResponseLossInput<'_>) -> Vec<String> {
    let reaped: std::collections::HashSet<&str> =
        input.reaped_ids.iter().map(String::as_str).collect();
    let mut lost: Vec<String> = Vec::new();
    for id in input.directive_ids {
        if id.is_empty() || !reaped.contains(id.as_str()) {
            continue;
        }
        let materialized = content_has_re_heading_for_id(input.content, id)
            || input
                .archives
                .iter()
                .any(|archive| content_has_re_heading_for_id(archive, id));
        if materialized {
            continue;
        }
        if !lost.iter().any(|existing| existing == id) {
            lost.push(id.clone());
        }
    }
    lost
}

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

    fn loss_input<'a>(
        directive_ids: &'a [String],
        reaped_ids: &'a [String],
        content: &'a str,
        archives: &'a [String],
    ) -> ReapedResponseLossInput<'a> {
        ReapedResponseLossInput {
            directive_ids,
            reaped_ids,
            content,
            archives,
        }
    }

    #[test]
    fn reaped_directive_response_loss_flags_reap_only_loss() {
        let directive = vec!["lostresp".to_string()];
        let reaped = vec!["lostresp".to_string()];
        let content = "### Re: prior — gpt-5\n\nAnswered something else.\n";
        let archives: Vec<String> = Vec::new();
        assert_eq!(
            reaped_directive_ids_without_response(&loss_input(
                &directive, &reaped, content, &archives
            )),
            vec!["lostresp".to_string()],
        );
    }

    #[test]
    fn reaped_directive_response_loss_flags_captured_but_id_lost() {
        let directive = vec!["kept".to_string(), "lost".to_string()];
        let reaped = vec!["kept".to_string(), "lost".to_string()];
        let content = "### Re: do #kept — opus-4-8\n\nShipped the kept fix.\n";
        let archives: Vec<String> = Vec::new();
        assert_eq!(
            reaped_directive_ids_without_response(&loss_input(
                &directive, &reaped, content, &archives
            )),
            vec!["lost".to_string()],
        );
    }

    #[test]
    fn reaped_directive_response_loss_accepts_archive_materialization() {
        let directive = vec!["archived".to_string()];
        let reaped = vec!["archived".to_string()];
        let content = "### Re: prior — gpt-5\n\nUnrelated live response.\n";
        let archives = vec!["### Re: do #archived — opus-4-8\n\nShipped earlier.\n".to_string()];
        assert!(
            reaped_directive_ids_without_response(&loss_input(
                &directive, &reaped, content, &archives
            ))
            .is_empty(),
            "a reaped id materialized in a HEAD compact archive is not a loss"
        );
    }

    #[test]
    fn directive_response_source_resolves_found_and_source() {
        let archives = vec!["### Re: do #archived — opus-4-8\n\nShipped earlier.\n".to_string()];

        let exchange = "### Re: do #live — opus-4-8\n\nShipped the live fix.\n";
        assert_eq!(
            directive_response_source(exchange, &archives, "live"),
            Some(ResponseSource::Exchange)
        );

        let unrelated = "### Re: prior — gpt-5\n\nUnrelated live response.\n";
        assert_eq!(
            directive_response_source(unrelated, &archives, "archived"),
            Some(ResponseSource::Archive)
        );
        assert!(directive_response_source(unrelated, &archives, "lost").is_none());
    }

    #[test]
    fn reaped_directive_response_loss_ignores_unreaped_directive() {
        let directive = vec!["pending".to_string()];
        let reaped: Vec<String> = Vec::new();
        let content = "### Re: prior — gpt-5\n\nAnswered.\n";
        let archives: Vec<String> = Vec::new();
        assert!(
            reaped_directive_ids_without_response(&loss_input(
                &directive, &reaped, content, &archives
            ))
            .is_empty(),
            "an unreaped directive id is not a loss"
        );
    }

    #[test]
    fn reaped_directive_response_loss_pins_multi_directive_single_heading_shape() {
        let directive = vec!["a".to_string(), "b".to_string()];
        let reaped = vec!["a".to_string(), "b".to_string()];
        let single_heading = "### Re: do #a — opus-4-8\n\nFixed #a; also addressed #b inline.\n";
        let archives: Vec<String> = Vec::new();
        assert_eq!(
            reaped_directive_ids_without_response(&loss_input(
                &directive,
                &reaped,
                single_heading,
                &archives
            )),
            vec!["b".to_string()],
            "documents the multi-directive-single-heading false positive"
        );

        let grouped_heading = "### Re: do #a, #b — opus-4-8\n\nFixed both.\n";
        assert!(
            reaped_directive_ids_without_response(&loss_input(
                &directive,
                &reaped,
                grouped_heading,
                &archives
            ))
            .is_empty(),
            "a grouped heading naming both ids is not a loss"
        );
    }

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
