//! Queue response heading and prompt identity matching.
//!
//! This module owns pure response-to-queue-head matching policy. Callers provide
//! already-read document/response text; file IO and lifecycle mutations stay in
//! orchestration.

use anyhow::{Context, Result};

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

/// First non-empty, trimmed line of `text`, or `None` when blank.
pub fn first_nonempty_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).find(|l| !l.is_empty())
}

/// Format consumed queue prompt(s) as a labeled blockquote echo so the response
/// block records the prompt it answered (#queue-prompt-echo-in-response).
///
/// `max_chars` is the opt-in `#queue-prompt-echo-summary` threshold: when
/// `Some(n)` and a prompt exceeds `n` characters, the echo records a bounded
/// summary (first line truncated + elided-char count + a pointer to the full
/// `agent:queue` text) instead of the verbatim prompt. `None` (default)
/// preserves the verbatim copy the user asked to keep "for now".
pub fn format_consumed_prompt_echo(consumed_texts: &[String], max_chars: Option<usize>) -> String {
    let mut out = String::from("> **Queue prompt:**\n>\n");
    let mut first_block = true;
    for text in consumed_texts {
        if text.trim().is_empty() {
            continue;
        }
        if !first_block {
            out.push_str(">\n");
        }
        first_block = false;
        let rendered = match max_chars {
            Some(limit) if text.chars().count() > limit => summarize_consumed_prompt(text, limit),
            _ => text.clone(),
        };
        for line in rendered.lines() {
            if line.trim().is_empty() {
                out.push_str(">\n");
            } else {
                out.push_str("> ");
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out
}

/// `#queue-prompt-echo-summary`: a bounded one-line summary of a long consumed
/// queue prompt: its first non-empty line truncated to `limit` characters on a
/// char boundary, plus how many characters were elided and a pointer to the full
/// text preserved in `agent:queue`.
pub fn summarize_consumed_prompt(text: &str, limit: usize) -> String {
    let total = text.chars().count();
    let first = first_nonempty_line(text).unwrap_or("").trim();
    let head: String = first.chars().take(limit).collect();
    let elided = total.saturating_sub(head.chars().count());
    format!("{head}… (+{elided} more chars; full prompt retained in agent:queue)")
}

pub fn line_is_response_heading(trimmed: &str) -> bool {
    trimmed == "## Assistant"
        || trimmed.starts_with("### Re:")
        || trimmed.starts_with("#### Re:")
        || trimmed.starts_with("##### Re:")
        || trimmed.starts_with("###### Re:")
}

/// Normalize a prompt line for "already present in exchange" comparison:
/// trim and strip a leading `❯` prompt marker.
pub fn normalize_prompt_line(line: &str) -> String {
    line.trim().trim_start_matches('❯').trim().to_string()
}

fn active_queue_head_text(content: &str) -> Result<Option<String>> {
    let (fm, _) = agent_doc_frontmatter::frontmatter::parse(content)?;
    if fm.queue_active != Some(true) {
        return Ok(None);
    }
    let components = agent_doc_element::element::parse(content)?;
    let comp = components
        .iter()
        .find(|component| component.name == "queue")
        .ok_or_else(|| {
            anyhow::anyhow!(
                "queue consume guard: queue_active is true but document has no agent:queue component"
            )
        })?;
    let body = &content[comp.open_end..comp.close_start];
    let entries = crate::document_queue::parse(body)
        .context("queue consume guard: failed to parse document queue")?;
    Ok(crate::document_queue::first_prompt(&entries).map(|prompt| prompt.text.clone()))
}

/// True when `head_id` names an item tracked in `agent:backlog`, `agent:review`,
/// or `agent:pending`.
pub fn head_id_names_tracked_directive_item(content: &str, head_id: &str) -> bool {
    let Ok(comps) = agent_doc_element::element::parse(content) else {
        return false;
    };
    comps
        .iter()
        .filter(|c| c.name == "backlog" || c.name == "review" || c.name == "pending")
        .any(|comp| {
            let (_, items, _) =
                agent_doc_element_backlog::backlog::parse_items(comp.content(content));
            items
                .iter()
                .any(|item| !item.id.is_empty() && item.id.eq_ignore_ascii_case(head_id))
        })
}

/// True when `head_id` matches a registered `prompt_presets` key in the
/// document frontmatter and does NOT also name a tracked directive item.
pub fn head_id_is_registered_preset(content: &str, head_id: &str) -> bool {
    if head_id_names_tracked_directive_item(content, head_id) {
        return false;
    }
    let Ok((fm, _)) = agent_doc_frontmatter::frontmatter::parse(content) else {
        return false;
    };
    agent_doc_frontmatter::frontmatter::resolve_prompt_preset_key(&fm.prompt_presets, head_id)
        .is_some()
}

/// A queue head that is just a `do [#id]` / `do #id` directive: the `do` verb
/// plus the id (with optional bracket sugar) and nothing else.
pub fn queue_head_is_bare_do_directive(queue_head: &str) -> bool {
    let norm = normalize_queue_prompt_text(queue_head);
    let Some(rest) = norm.strip_prefix("do ") else {
        return false;
    };
    matches!(
        rest.strip_prefix('#'),
        Some(id)
            if !id.is_empty()
                && id
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    )
}

fn normalized_queue_prompt_text_is_queue_activation_trigger(normalized: &str) -> bool {
    let after = normalized
        .strip_prefix("do queue")
        .or_else(|| normalized.strip_prefix("run queue"));
    let Some(after) = after else {
        return false;
    };
    after.is_empty() || after.starts_with(|c: char| !c.is_alphanumeric() && c != '#')
}

/// True when `text` is the queue activation prompt form (`do queue` /
/// `run queue`) after queue prompt normalization strips prompt markers and pins.
pub fn queue_prompt_text_is_queue_activation_trigger(text: &str) -> bool {
    let normalized = normalize_queue_prompt_text(text);
    normalized_queue_prompt_text_is_queue_activation_trigger(&normalized)
}

/// True when `text` is a free-text queue prompt (NOT an id-backed directive and
/// NOT a queue activation trigger).
pub fn queue_prompt_text_is_free_text(content: &str, text: &str) -> bool {
    let normalized = normalize_queue_prompt_text(text);
    if let Some(ids) = crate::queue_directive::topic_resolves_to_only_id_directives(&normalized) {
        return ids
            .iter()
            .all(|id| head_id_is_registered_preset(content, id));
    }
    !normalized_queue_prompt_text_is_queue_activation_trigger(&normalized)
}

/// True when the active queue head is a free-text prompt: it is neither an
/// id-backed directive nor a queue activation trigger.
pub fn queue_head_is_free_text_prompt(content: &str) -> Result<bool> {
    let Some(queue_head) = active_queue_head_text(content)? else {
        return Ok(false);
    };
    Ok(queue_prompt_text_is_free_text(content, &queue_head))
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

/// Locate, within `region` (the exchange content), the byte offset of the line
/// where this cycle's response heading begins. Prefers the captured response's
/// first line; falls back to the last non-code `### Re:` heading. `region_base`
/// is the absolute offset of `region` within the full document, used to skip
/// matches inside fenced code blocks.
pub fn locate_response_heading_offset(
    region: &str,
    region_base: usize,
    response_first_line: Option<&str>,
    code_ranges: &[(usize, usize)],
) -> Option<usize> {
    let in_code = |rel: usize| {
        let abs = region_base + rel;
        code_ranges.iter().any(|&(cs, ce)| abs >= cs && abs < ce)
    };

    if let Some(target) = response_first_line.map(str::trim).filter(|t| !t.is_empty()) {
        let mut offset = 0usize;
        for line in region.split_inclusive('\n') {
            if line.trim() == target && !in_code(offset) {
                return Some(offset);
            }
            offset += line.len();
        }
    }

    let mut offset = 0usize;
    let mut found = None;
    for line in region.split_inclusive('\n') {
        if line_is_response_heading(line.trim()) && !in_code(offset) {
            found = Some(offset);
        }
        offset += line.len();
    }
    found
}

/// Embed the consumed queue prompt echo immediately after this cycle's response
/// heading inside the `exchange` component. Returns `content` unchanged (fail-safe)
/// when the exchange/heading cannot be located, the prompt is empty, or the prompt
/// already appears in the exchange (e.g. a user typed it in directly).
pub fn embed_consumed_prompt_in_response(
    content: &str,
    consumed_texts: &[String],
    response_first_line: Option<&str>,
) -> String {
    if consumed_texts.iter().all(|t| t.trim().is_empty()) {
        return content.to_string();
    }
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return content.to_string();
    };
    let Some(exchange) = components.iter().find(|c| c.name == "exchange") else {
        return content.to_string();
    };
    let region = &content[exchange.open_end..exchange.close_start];

    // Idempotency / manual-turn dedup: if the prompt's first line already appears
    // as an exchange line (user typed it, or a prior echo exists), skip injection.
    // #queue-prompt-echo-summary: the opt-in length threshold is read from the
    // document's own frontmatter (default None = verbatim copy).
    let max_chars = agent_doc_frontmatter::frontmatter::parse(content)
        .ok()
        .and_then(|(fm, _)| fm.queue_prompt_echo_max_chars);
    let echo = format_consumed_prompt_echo(consumed_texts, max_chars);
    if region.contains(echo.trim_end()) {
        return content.to_string();
    }
    let already_present = consumed_texts
        .iter()
        .filter_map(|t| first_nonempty_line(t))
        .any(|first| {
            let needle = normalize_prompt_echo_presence_line(first);
            !needle.is_empty()
                && region.lines().any(|line| {
                    normalize_prompt_line(line) == needle
                        || normalize_prompt_echo_presence_line(line) == needle
                })
        });
    if already_present {
        return content.to_string();
    }

    let code_ranges = agent_doc_element::element::find_code_ranges(content);
    let Some(heading_rel) = locate_response_heading_offset(
        region,
        exchange.open_end,
        response_first_line,
        &code_ranges,
    ) else {
        return content.to_string();
    };
    let Some(nl) = region[heading_rel..].find('\n') else {
        return content.to_string();
    };
    let insert_abs = exchange.open_end + heading_rel + nl + 1;

    let mut result = String::with_capacity(content.len() + echo.len() + 2);
    result.push_str(&content[..insert_abs]);
    result.push('\n');
    result.push_str(&echo);
    result.push('\n');
    result.push_str(&content[insert_abs..]);
    result
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

    fn active_queue_doc(head: &str) -> String {
        format!(
            concat!(
                "---\nqueue_active: true\n---\n\n",
                "<!-- agent:queue auto -->\n",
                "- {}\n",
                "<!-- /agent:queue -->\n",
            ),
            head
        )
    }

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
    fn queue_head_is_bare_do_directive_detection() {
        assert!(queue_head_is_bare_do_directive("do [#foo]"));
        assert!(queue_head_is_bare_do_directive("do #foo"));
        assert!(queue_head_is_bare_do_directive(":pushpin: do [#foo]"));
        assert!(queue_head_is_bare_do_directive(":round_pushpin: do #foo"));
        assert!(!queue_head_is_bare_do_directive(
            "JB Run Agent Doc on tsift.md add the prompt into agent:queue.\n#spec-test-build-install-commit-push"
        ));
        assert!(!queue_head_is_bare_do_directive(
            "#spec-test-build-install-commit-push"
        ));
    }

    #[test]
    fn queue_prompt_text_is_queue_activation_trigger_recognizes_prompt_forms() {
        assert!(queue_prompt_text_is_queue_activation_trigger("do queue"));
        assert!(queue_prompt_text_is_queue_activation_trigger("run queue"));
        assert!(queue_prompt_text_is_queue_activation_trigger("Do Queue."));
        assert!(queue_prompt_text_is_queue_activation_trigger(
            "❯ :pushpin: run queue"
        ));
        assert!(!queue_prompt_text_is_queue_activation_trigger(
            "do #queue Phase 2"
        ));
        assert!(!queue_prompt_text_is_queue_activation_trigger(
            "document queue behavior"
        ));
    }

    #[test]
    fn queue_prompt_text_is_free_text_classification() {
        let content =
            "---\nqueue_active: true\n---\n<!-- agent:queue -->\n- x\n<!-- /agent:queue -->\n";
        assert!(!queue_prompt_text_is_free_text(
            content,
            "do [#fullboundary]"
        ));
        assert!(!queue_prompt_text_is_free_text(content, "#orphanqhead"));
        assert!(!queue_prompt_text_is_free_text(content, "do queue"));
        assert!(queue_prompt_text_is_free_text(
            content,
            "My free-text queue items are not immediately struck as if they are addressed."
        ));
        assert!(queue_prompt_text_is_free_text(
            content,
            "Approve [#shoptiers] then ship it"
        ));
    }

    #[test]
    fn queue_head_is_free_text_prompt_classifies_head_shapes() {
        assert!(
            queue_head_is_free_text_prompt(&active_queue_doc(
                "Is tsift properly integrated into multi-crate architecture?"
            ))
            .unwrap(),
            "a no-#id queue head is free text and consumable by being answered"
        );
        assert!(
            !queue_head_is_free_text_prompt(&active_queue_doc(":pushpin: do [#foo]")).unwrap(),
            "a pinned do[#id] head is id-backed, not free text"
        );
        for head in [
            "[#foo]",
            ":pushpin: [#foo]",
            ":round_pushpin: [#foo]",
            "do [#syncbarrier] [#crdtsvdom]",
        ] {
            assert!(
                !queue_head_is_free_text_prompt(&active_queue_doc(head)).unwrap(),
                "head {head:?} is id-backed, not free text"
            );
        }
        assert!(
            queue_head_is_free_text_prompt(&active_queue_doc(
                "Approve [#shoptiers]. What are #next-steps?"
            ))
            .unwrap(),
            "a free-text head that merely mentions #ids stays free text"
        );
        assert!(
            !queue_head_is_free_text_prompt(&active_queue_doc("do queue")).unwrap(),
            "queue activation triggers are not free-text heads"
        );
        let inactive = active_queue_doc("Review the queue diagnostics")
            .replace("queue_active: true", "queue_active: false");
        assert!(!queue_head_is_free_text_prompt(&inactive).unwrap());
    }

    #[test]
    fn queue_head_is_free_text_prompt_registered_presets_are_synthetic() {
        let registered = concat!(
            "---\nqueue_active: true\n",
            "prompt_presets:\n",
            "  '#advance-review': Go through the review items.\n",
            "---\n\n",
            "<!-- agent:queue auto -->\n",
            "- #advance-review\n",
            "<!-- /agent:queue -->\n",
        );
        assert!(queue_head_is_free_text_prompt(registered).unwrap());
        assert!(head_id_is_registered_preset(registered, "advance-review"));

        let preset_and_tracked = concat!(
            "---\nqueue_active: true\n",
            "prompt_presets:\n",
            "  '#advance-review': Go through the review items.\n",
            "---\n\n",
            "<!-- agent:queue auto -->\n",
            "- #advance-review\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#advance-review] tracked directive that shadows the preset name\n",
            "<!-- /agent:backlog -->\n",
        );
        assert!(!queue_head_is_free_text_prompt(preset_and_tracked).unwrap());
        assert!(!head_id_is_registered_preset(
            preset_and_tracked,
            "advance-review"
        ));

        assert!(!queue_head_is_free_text_prompt(&active_queue_doc("#advance-review")).unwrap());
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

    #[test]
    fn queue_prompt_echo_copies_verbatim_when_threshold_is_none() {
        // #queue-prompt-echo-summary: default (None) preserves the verbatim copy
        // the user asked to keep "for now".
        let long = "do [#x] ".to_string() + &"word ".repeat(100);
        let echo = format_consumed_prompt_echo(std::slice::from_ref(&long), None);
        assert!(echo.starts_with("> **Queue prompt:**\n>\n"));
        assert!(echo.contains(long.trim_end()));
        assert!(!echo.contains("more chars"));
    }

    #[test]
    fn queue_prompt_echo_copies_verbatim_when_under_threshold() {
        let short = "do [#x] short prompt".to_string();
        let echo = format_consumed_prompt_echo(std::slice::from_ref(&short), Some(200));
        assert!(echo.contains("> do [#x] short prompt"));
        assert!(!echo.contains("more chars"));
    }

    #[test]
    fn queue_prompt_echo_summarizes_when_over_threshold() {
        let long = "First line is the gist.\n".to_string() + &"tail ".repeat(100);
        let echo = format_consumed_prompt_echo(std::slice::from_ref(&long), Some(40));
        // The verbatim tail must NOT appear; a bounded summary must.
        assert!(!echo.contains(&"tail ".repeat(100)));
        assert!(echo.contains("First line is the gist."));
        assert!(echo.contains("more chars; full prompt retained in agent:queue"));
        // Summary is a single quoted line plus the label.
        assert_eq!(echo.matches("more chars").count(), 1);
    }

    #[test]
    fn queue_prompt_echo_summary_truncates_first_line_on_char_boundary() {
        // Multibyte content must not panic and must truncate on a char boundary.
        let text = "héllo wörld ".repeat(20);
        let summary = summarize_consumed_prompt(&text, 5);
        assert!(summary.starts_with("héllo"));
        assert!(summary.contains("more chars"));
    }

    #[test]
    fn queue_prompt_echo_embedding_skips_stale_blockquoted_echo_variant() {
        let prompt = ":pushpin: Fix the root cause of this issue that occurred in this document.";
        let content = format!(
            concat!(
                "---\nqueue_active: true\n---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Re: root fix\n\n",
                "> **Queue prompt:**\n>\n",
                "> Fix the root cause of this issue that occurred in this document.\n\n",
                "Handled once.\n",
                "<!-- /agent:exchange -->\n\n",
                "<!-- agent:queue priority go -->\n",
                "- {prompt}\n",
                "<!-- /agent:queue -->\n",
            ),
            prompt = prompt
        );

        let updated = embed_consumed_prompt_in_response(
            &content,
            &[prompt.to_string()],
            Some("### Re: root fix"),
        );

        assert_eq!(
            updated, content,
            "a stale blockquoted queue-prompt echo with the priority marker stripped must not be reinserted"
        );
        assert_eq!(updated.matches("> **Queue prompt:**").count(), 1);

        let one_line_echo = content.replace(
            "> **Queue prompt:**\n>\n> Fix the root cause of this issue that occurred in this document.",
            "> **Queue prompt:** Fix the root cause of this issue that occurred in this document.",
        );
        let updated = embed_consumed_prompt_in_response(
            &one_line_echo,
            &[prompt.to_string()],
            Some("### Re: root fix"),
        );
        assert_eq!(
            updated, one_line_echo,
            "legacy one-line queue-prompt echoes must also count as already present"
        );
        assert_eq!(updated.matches("> **Queue prompt:**").count(), 1);
    }
}
