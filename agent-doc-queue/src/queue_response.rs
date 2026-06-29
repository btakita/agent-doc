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

/// Collapse a string to lowercase alphanumeric words separated by single spaces.
/// Every non-alphanumeric run (`:pushpin:`, `- `, backticks, punctuation,
/// newlines) becomes one space, so two spellings of the same prompt compare equal
/// regardless of cosmetic markers (#ftstrike).
pub fn normalize_for_answer_match(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for ch in s.chars() {
        if ch.is_alphanumeric() {
            for lc in ch.to_lowercase() {
                out.push(lc);
            }
            prev_space = false;
        } else if !prev_space && !out.is_empty() {
            out.push(' ');
            prev_space = true;
        }
    }
    out.trim().to_string()
}

/// The concatenated, normalized text of every `>` blockquote line in the response.
/// The skill quotes the prompt it is answering as a blockquote (`> **Queue
/// prompt:**` / `> <text>`), so a free-text head is "answered" only when its text
/// appears in this quoted region -- prose that merely *mentions* a head (without
/// quoting it as a prompt) does NOT count, which keeps an unaddressed operator
/// report from being silently struck (#ftstrike false-strike guard).
fn response_blockquote_text(response_body: &str) -> String {
    let joined = response_body
        .lines()
        .map(str::trim_start)
        .filter(|line| line.starts_with('>'))
        .map(|line| line.trim_start_matches('>'))
        .collect::<Vec<_>>()
        .join(" ");
    normalize_for_answer_match(&joined)
}

fn strip_echo_presence_list_marker(line: &str) -> &str {
    let trimmed = line.trim_start();
    if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
    {
        return rest.trim_start();
    }
    trimmed
}

fn strip_echo_presence_checkbox_marker(line: &str) -> &str {
    let Some(rest) = line.strip_prefix('[') else {
        return line;
    };
    let Some(rest) = rest.strip_prefix(|ch: char| ch == ' ' || ch == 'x' || ch == 'X') else {
        return line;
    };
    rest.strip_prefix("] ").unwrap_or(line).trim_start()
}

fn normalize_prompt_echo_presence_line(line: &str) -> String {
    let mut text = line.trim();
    while let Some(rest) = text.strip_prefix('>') {
        text = rest.trim_start();
    }
    if let Some(rest) = text
        .strip_prefix("**Queue prompt:**")
        .or_else(|| text.strip_prefix("**Queue prompts:**"))
    {
        text = rest.trim_start();
    }
    text = text.trim_start_matches('❯').trim_start();
    text = strip_echo_presence_list_marker(text);
    text = strip_echo_presence_checkbox_marker(text);
    crate::document_queue::strip_priority_markers(text)
        .trim()
        .to_string()
}

/// True when a response contains an explicit `> **Queue prompt:**` echo whose
/// normalized line exactly matches `head_text`. This is the conservative short
/// prompt path: a one-word head like `deploy` is proof only when it appears in
/// the labeled queue-prompt echo, not just anywhere in assistant prose.
fn response_explicit_queue_prompt_echoes_head(response_body: &str, head_text: &str) -> bool {
    let head_clean = crate::document_queue::strip_priority_markers(head_text);
    let head_norm = normalize_for_answer_match(&free_text_head_match_prose(&head_clean));
    if head_norm.is_empty() {
        return false;
    }

    let mut in_queue_prompt_echo = false;
    for line in response_body.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with('>') {
            if !trimmed.trim().is_empty() {
                in_queue_prompt_echo = false;
            }
            continue;
        }

        let quoted = trimmed.trim_start_matches('>').trim_start();
        let candidate = if let Some(rest) = quoted
            .strip_prefix("**Queue prompt:**")
            .or_else(|| quoted.strip_prefix("**Queue prompts:**"))
        {
            in_queue_prompt_echo = true;
            rest.trim_start()
        } else if in_queue_prompt_echo {
            quoted
        } else {
            continue;
        };

        let candidate = normalize_prompt_echo_presence_line(candidate);
        if candidate.is_empty() {
            continue;
        }
        if normalize_for_answer_match(&candidate) == head_norm {
            return true;
        }
    }
    false
}

/// True when a queue head's text carries the in-progress marker at its head
/// (after optional leading whitespace). The binary stamps this marker on the
/// cycle's drain target during preflight queue maintenance
/// (`set_first_prompt_in_progress`), so it is the binary's own authoritative record
/// of "the head this cycle is working" -- used by `#qheadstrikeauto` to auto-strike
/// an answered free-text drain target without depending on agent prose formatting.
pub fn head_carries_in_progress_marker(text: &str) -> bool {
    text.trim_start()
        .starts_with(crate::document_queue::IN_PROGRESS_MARKER)
}

/// The prose prefix of a free-text queue head used for answer-matching: every
/// line before the first fenced code block (` ``` ` or `~~~`). A head whose body
/// is dominated by a pasted console/route log (the common shape of an operator
/// bug report) is answered by quoting its prose lead, never the whole log, so
/// matching on the *entire* normalized node text (`#ftstrike-fence`) could never
/// strike it -- the response blockquote can't possibly `contains` the full log.
/// Matching on the prose prefix instead lets a code-fenced report strike when its
/// lead is quoted. Falls back to the whole text when there is no fence.
pub fn free_text_head_match_prose(head_text: &str) -> String {
    let mut prose = String::new();
    for line in head_text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            break;
        }
        prose.push_str(line);
        prose.push('\n');
    }
    prose
}

/// True when the committed `response_body` answers the free-text queue head
/// `head_text`: the head's normalized **prose prefix** (text before any fenced
/// code block -- see [`free_text_head_match_prose`]) appears inside the response's
/// quoted-prompt blockquote region. Requires a prose prefix of at least four
/// significant words so a short/empty head cannot match incidentally -- the
/// conservative direction, because a false positive silently drops an unaddressed
/// operator report.
pub fn free_text_head_answered_by_response(response_body: &str, head_text: &str) -> bool {
    // Strip the leading operator/agent pin (`:pushpin:` ...) first -- its literal
    // shortcode word would otherwise survive normalization and break the match.
    let head_clean = crate::document_queue::strip_priority_markers(head_text);
    if response_explicit_queue_prompt_echoes_head(response_body, &head_clean) {
        return true;
    }
    // `#ftstrike-fence`: match on the prose prefix, not the full node text -- a head
    // whose body is a pasted log is only ever quoted by its lead line(s).
    let head_prose = free_text_head_match_prose(&head_clean);
    let head_norm = normalize_for_answer_match(&head_prose);
    if head_norm.split(' ').filter(|w| !w.is_empty()).count() < 4 {
        return false;
    }
    response_blockquote_text(response_body).contains(&head_norm)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FTSTRIKE_RESPONSE: &str = concat!(
        "### Re: two reports -- opus\n\n",
        "> **Queue prompts:**\n",
        "> - JB `Run Agent Doc` is stalled on this document when I tried to start the queue run. No notification.\n",
        "> - My free-text queue items are not immediately struck as if they are addressed.\n\n",
        "Triaged both.\n",
    );

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

    #[test]
    fn free_text_head_answered_by_response_when_quoted_in_response_blockquote() {
        assert!(free_text_head_answered_by_response(
            FTSTRIKE_RESPONSE,
            "My free-text queue items are not immediately struck as if they are addressed."
        ));
        // Cosmetic differences (`:pushpin:`, leading `- `, backticks) must not matter.
        assert!(free_text_head_answered_by_response(
            FTSTRIKE_RESPONSE,
            ":pushpin: JB Run Agent Doc is stalled on this document when I tried to start the queue run. No notification."
        ));
    }

    #[test]
    fn free_text_head_answered_by_response_ignores_prose_mentions() {
        // FALSE-STRIKE GUARD: a head whose text appears only in prose (not a `>`
        // quoted-prompt blockquote) must NOT be considered answered -- otherwise an
        // unaddressed operator report would be silently struck/dropped.
        let prose_only = concat!(
            "### Re: something else -- opus\n\n",
            "I noticed my free-text queue items are not immediately struck as if they are addressed, ",
            "but that is a different report I did not handle this turn.\n",
        );
        assert!(!free_text_head_answered_by_response(
            prose_only,
            "My free-text queue items are not immediately struck as if they are addressed."
        ));
    }

    #[test]
    fn free_text_head_answered_by_response_rejects_short_unlabeled_heads() {
        let resp = "### Re: x\n\n> - fix it now\n";
        assert!(!free_text_head_answered_by_response(resp, "fix it now"));
    }

    #[test]
    fn free_text_head_answered_by_response_accepts_short_explicit_queue_prompt_echo() {
        let labeled = "### Re: deploy\n\n> **Queue prompt:**\n>\n> deploy\n\nDone.\n";
        assert!(
            free_text_head_answered_by_response(labeled, "deploy"),
            "a short head is proof when it is the explicit queue-prompt echo"
        );

        let one_line = "### Re: deploy\n\n> **Queue prompt:** deploy\n\nDone.\n";
        assert!(
            free_text_head_answered_by_response(one_line, ":pushpin: deploy"),
            "same-line queue-prompt echo should also prove a pinned short head"
        );

        let unlabeled = "### Re: deploy\n\n> deploy\n\nDone.\n";
        assert!(
            !free_text_head_answered_by_response(unlabeled, "deploy"),
            "an unlabeled blockquote is not enough proof for a short head"
        );
    }

    #[test]
    fn free_text_head_answered_by_response_matches_code_fenced_prose_lead() {
        // #ftstrike-fence regression: an operator bug report whose body is a short
        // prose lead followed by a pasted console/route log. The response quotes ONLY
        // the prose lead as a blockquote (nobody quotes the whole log), so matching on
        // the full normalized node text never struck it. Matching on the prose prefix
        // must now strike it.
        let head = concat!(
            "JB `Run Agent Doc` on sampleportal.md did not submit\n",
            "```\n",
            "claude exited cleanly.\n",
            "Press Enter to restart, or 'q' to exit.\n",
            "[agent-doc] auto-trigger: timed out waiting for claude prompt\n",
            "```",
        );
        let response = concat!(
            "### Re: did not submit -- opus\n\n",
            "> **Queue prompt:**\n",
            "> JB `Run Agent Doc` on sampleportal.md did not submit.\n\n",
            "Triaged.\n",
        );
        assert!(
            free_text_head_answered_by_response(response, head),
            "a code-fenced report quoted by its prose lead must count as answered"
        );
        // Prose prefix is just the lead line, not the whole log.
        assert_eq!(
            free_text_head_match_prose(head).trim(),
            "JB `Run Agent Doc` on sampleportal.md did not submit"
        );
        // FALSE-STRIKE GUARD: a head that is ALL log (no prose lead) has an empty
        // prose prefix and must never match.
        let log_only = "```\nsome pasted log line one\nsome pasted log line two\n```";
        assert!(!free_text_head_answered_by_response(response, log_only));
    }
}
