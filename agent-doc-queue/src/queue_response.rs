//! Queue response heading and prompt identity matching.
//!
//! This module owns pure response-to-queue-head matching policy. Callers provide
//! already-read document/response text; file IO and lifecycle mutations stay in
//! orchestration.

use agent_doc_document::queue_projection::strip_priority_markers;
use anyhow::Result;
use std::borrow::Cow;

/// Text that may carry direct coordinator answer evidence.
///
/// Binary-owned subagent completion cells quote the worker's response. That
/// response commonly carries its own `> **Queue prompt:**` echo, but it is a
/// handoff awaiting coordinator integration, not the coordinator's answer.
/// Keep those cells out of every response-evidence matcher while preserving
/// heading-shaped prose that lacks the binary attestation.
fn direct_response_text(text: &str) -> Cow<'_, str> {
    if agent_doc_document::subagent_completion::completion_cell_line_ranges(text).is_empty() {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(agent_doc_document::subagent_completion::strip_completion_cells(text))
    }
}

pub fn queue_prompt_done_id(text: &str) -> Option<String> {
    // `#claimrowtombstone`: lifecycle markers and the binary-owned
    // `[#owner#generation]` attempt annotation are projection metadata, never
    // the row's id. Reading the first `#` of the raw row named a claimed
    // `☑️ [#subagent:x#1] do [#x]` row `subagent`, so the operator-delete
    // tombstone ledger recorded `x` as deleted the moment its claim projected,
    // and lifecycle convergence then dropped the live claimed row.
    //
    // `#taggedidhead`: a leading intent/preset tag routes the work, it does
    // not name it. `#review [#adr0043-publish]` is claimed, reported and
    // released as `adr0043-publish` (`queue_head_identity`,
    // `#claimdispatchidentity`), so its done id is that id too. Reading the
    // first `#` named it `review`: `queue consume --id adr0043-publish` found
    // no head, `--done adr0043-publish` left the row, and the reported `☑️`
    // row had no closing command at all (haiven-dev sdk-demo.md, 2026-10-28).
    // Only an id-backed work identity overrides; a free-text identity keeps
    // the first-`#` reading below.
    if let agent_doc_element_queue::QueueItemIdentity::Id(id) =
        agent_doc_element_queue::queue_head_identity(text)
    {
        return Some(id.to_ascii_lowercase());
    }
    let visible = strip_priority_markers(text);
    let marker = visible.find('#')?;
    let tail = &visible[marker + 1..];
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
            strip_priority_markers(line)
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

/// Return the id from a head whose complete visible text is a bare `#id`.
///
/// Keep this narrower than the general id-directive grammar: explicit
/// `do [#id]` and bracketed `[#id]` heads retain their directive semantics.
fn bare_hash_reference_id(text: &str) -> Option<String> {
    let text = text.trim().trim_start_matches('❯').trim();
    let text = strip_priority_markers(text);
    let id = text.strip_prefix('#')?;
    if id.is_empty()
        || !id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        return None;
    }
    Some(id.to_ascii_lowercase())
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
    let Ok((fm, _)) = agent_doc_frontmatter::frontmatter::parse(content) else {
        return false;
    };
    head_id_is_registered_preset_with_presets(content, head_id, &fm.prompt_presets)
}

/// [`head_id_is_registered_preset`] with a caller-resolved preset map.
pub fn head_id_is_registered_preset_with_presets(
    content: &str,
    head_id: &str,
    presets: &agent_doc_frontmatter::frontmatter::PromptPresets,
) -> bool {
    if head_id_names_tracked_directive_item(content, head_id) {
        return false;
    }
    agent_doc_frontmatter::frontmatter::resolve_prompt_preset_key(presets, head_id).is_some()
}

/// Resolve every prompt preset referenced by a queue head.
pub fn queue_prompt_preset_expansions(content: &str, text: &str) -> Vec<(String, String)> {
    let Ok((fm, _)) = agent_doc_frontmatter::frontmatter::parse(content) else {
        return Vec::new();
    };
    queue_prompt_preset_expansions_with_presets(text, &fm.prompt_presets)
}

/// Resolve every prompt preset referenced by a queue head using a
/// caller-resolved map (including project-inherited defaults).
pub fn queue_prompt_preset_expansions_with_presets(
    text: &str,
    presets: &agent_doc_frontmatter::frontmatter::PromptPresets,
) -> Vec<(String, String)> {
    let text = strip_priority_markers(text);
    let text = crate::queue_continuation::mask_inline_code(&text);
    let referenced = text
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '#' | '-' | '_')))
        .filter(|token| token.starts_with('#'));
    let mut expansions = Vec::new();
    for token in referenced {
        let Some((name, body)) = presets
            .iter()
            .find(|(name, _)| token.eq_ignore_ascii_case(name))
        else {
            continue;
        };
        if expansions
            .iter()
            .any(|(seen, _): &(String, String)| seen.eq_ignore_ascii_case(name))
        {
            continue;
        }
        expansions.push((name.clone(), body.clone()));
    }
    expansions
}

/// True when the response contains evidence for every resolved preset expansion
/// requested by this queue head. A compact preset reference is not itself evidence.
///
/// `#ftstrikeclaimedmention` (agent-doc-bugs.md 2026-10-09): a preset body is
/// shared by every head that uses the preset, so the body alone cannot say WHICH
/// head was answered. A status response quoting `#gh-fix`'s body ("fix then
/// close") once answered all four queued `#gh-fix <url>` heads. The head's own
/// argument (its text with the `#tag` tokens removed, e.g. the issue URL) must
/// also appear in the response, matched on whole words so `.../issues/22` is not
/// proven by `.../issues/227`.
pub fn prompt_preset_head_answered_by_response(
    content: &str,
    response_body: &str,
    head_text: &str,
) -> bool {
    let response_body = direct_response_text(response_body);
    let expansions = queue_prompt_preset_expansions(content, head_text);
    if expansions.is_empty() {
        return false;
    }
    let response = normalize_for_answer_match(&response_body);
    let bodies_present = expansions.into_iter().all(|(_, body)| {
        let body = normalize_for_answer_match(&body);
        !body.is_empty() && response.contains(&body)
    });
    if !bodies_present {
        return false;
    }
    let argument = preset_head_argument(head_text);
    argument.is_empty() || format!(" {response} ").contains(&format!(" {argument} "))
}

/// The normalized argument of a preset-bearing queue head: the head text with
/// lifecycle markers and every `#tag` token (preset references, intent tags)
/// removed. Empty for a bare preset head such as `[#upgrade]`.
fn preset_head_argument(head_text: &str) -> String {
    let clean = strip_priority_markers(head_text);
    let mut kept = String::with_capacity(clean.len());
    let mut chars = clean.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '#' {
            while chars
                .peek()
                .is_some_and(|next| next.is_ascii_alphanumeric() || matches!(next, '-' | '_'))
            {
                chars.next();
            }
            kept.push(' ');
            continue;
        }
        kept.push(ch);
    }
    normalize_for_answer_match(&kept)
}

/// The completion proof for a free-text head, given whether an active worker
/// claim holds it (`#ftstrikeclaimedmention`).
///
/// An unclaimed head uses [`queue_head_answered_by_response`]. A CLAIMED head
/// is owned by another worker, so it accepts only
/// [`free_text_head_has_explicit_answer_evidence`]: an exact
/// `> **Queue prompt:**` echo of the head, not a bare listing. A prose mention,
/// a quote of the head's shared preset body, a `### Re:` identifier heading,
/// and a blockquote that merely starts with the head's prose are not evidence
/// for it. `#claimstrike` still holds: that exact echo strikes the claimed head
/// (the caller releases its claim) unless a deferral follows it.
///
/// Every strike site and the session-check residue guard (`#qheadresidue`)
/// must use this one predicate, so the strike set and the residue set agree.
pub fn queue_head_answered_by_response_with_claim(
    content: &str,
    response_body: &str,
    head_text: &str,
    claimed: bool,
) -> bool {
    if release_include_context_is_open(content, head_text) {
        return false;
    }
    if claimed {
        free_text_head_has_explicit_answer_evidence(response_body, head_text)
            || response_explicitly_echoes_nest_vector(content, response_body, head_text)
    } else {
        queue_head_answered_by_response(content, response_body, head_text)
    }
}

/// Canonical response-evidence rule for a free-text or resolved preset head.
///
/// Admission and consumption must call this same predicate: an exact labeled
/// queue-prompt echo is sufficient evidence even for a short preset token,
/// while a response that omits the echo may still prove a resolved preset by
/// reproducing its full expansion.
pub fn queue_head_answered_by_response(
    content: &str,
    response_body: &str,
    head_text: &str,
) -> bool {
    free_text_head_answered_in_document(content, response_body, head_text)
        || prompt_preset_head_answered_by_response(content, response_body, head_text)
}

/// Free-text answer proof with the document's nest vector (`#nestvector`).
///
/// A quote of a descendant's vector answers that descendant. It does not answer
/// an ancestor whose prose the vector merely starts with. Preset expansions are
/// not proof here.
pub fn free_text_head_answered_in_document(
    content: &str,
    response_body: &str,
    head_text: &str,
) -> bool {
    if release_include_context_is_open(content, head_text) {
        return false;
    }
    if response_explicitly_echoes_nest_vector(content, response_body, head_text) {
        return true;
    }
    if descendant_nest_vector_blocks_ancestor(content, response_body, head_text) {
        return false;
    }
    free_text_head_answered_by_response(response_body, head_text)
}

/// True when the active queue head is exactly the registered prompt preset id.
pub fn active_queue_head_is_registered_preset(content: &str, preset_id: &str) -> Result<bool> {
    let Some(head) = crate::queue_heads::active_queue_head_text(content)? else {
        return Ok(false);
    };
    let normalized = normalize_queue_prompt_text(&head);
    Ok(
        crate::queue_directive::topic_resolves_to_exact_id(&normalized, preset_id)
            && head_id_is_registered_preset(content, preset_id),
    )
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
    // GH #150: a bare `#id` with no open tracked-work owner is prose, not an
    // id-backed directive. Treating every hash token as id-backed made an
    // unknown token selectable while neither response closeout nor plain
    // `queue consume` could strike it. Explicit `do [#id]` / `[#id]` shapes
    // remain directives and the orphan-id escape hatch remains available.
    if let Some(id) = bare_hash_reference_id(text) {
        let normalized_id = agent_doc_element_backlog::backlog::normalize_pending_id(&id);
        return !agent_doc_element_backlog::backlog::open_tracked_work_ids_in_content(content)
            .iter()
            .any(|open_id| {
                agent_doc_element_backlog::backlog::normalize_pending_id(open_id) == normalized_id
            });
    }
    if let Some(ids) = crate::queue_directive::topic_resolves_to_only_id_directives(&normalized)
        .or_else(|| crate::queue_directive::explicit_do_directive_with_note_ids(&normalized))
    {
        return ids
            .iter()
            .all(|id| head_id_is_registered_preset(content, id));
    }
    if head_leads_with_open_backlog_directive(content, text) {
        return false;
    }
    !normalized_queue_prompt_text_is_queue_activation_trigger(&normalized)
}

/// GH #129: a head that LEADS with an id directive followed by prose —
/// `[#id] [operator-verify] Confirm whether ...`, `do [#id] and keep the old
/// API` — is id-backed when that id is still open `agent:backlog` work.
///
/// The exact-id grammar above only recognizes heads made of nothing but
/// directives, so these annotated heads fell through to free text, and the
/// answered-free-text strike (`#qheadresidue`) could complete one by a
/// response's quote or `### Re: #id` heading while its backlog item stayed
/// open. Every other surface already treats the head as id-backed: the
/// `#queue-clear-unrun-items` session-check guard (`do_directive_target_ids`)
/// then reported the struck head as silently dropped, and `#qheadannotation`
/// parses it as an annotated id head. Requiring the id to be open keeps a head
/// that merely starts with a resolved or unknown id consumable as free text.
fn head_leads_with_open_backlog_directive(content: &str, text: &str) -> bool {
    let Some(first_line) = text.lines().find(|line| !line.trim().is_empty()) else {
        return false;
    };
    // Only the leading directive id decides: a later `#id` in the prose is a
    // mention, not the head's identity.
    let Some(lead) = crate::queue_directive::do_directive_target_ids_in_line(first_line)
        .into_iter()
        .next()
    else {
        return false;
    };
    let lead = agent_doc_element_backlog::backlog::normalize_pending_id(&lead);
    agent_doc_element_backlog::backlog::open_backlog_ids_in_content(content)
        .iter()
        .any(|id| agent_doc_element_backlog::backlog::normalize_pending_id(id) == lead)
}

/// True when the active queue head is a free-text prompt: it is neither an
/// id-backed directive nor a queue activation trigger.
pub fn queue_head_is_free_text_prompt(content: &str) -> Result<bool> {
    let Some(queue_head) = crate::queue_heads::active_queue_head_text(content)? else {
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

pub fn response_explicitly_targets_queue_head(response: &str, queue_head: &str) -> bool {
    response
        .lines()
        .filter_map(response_heading_topic)
        .any(|topic| response_topic_matches_queue_head(topic, queue_head))
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

/// Normalized candidates for the weak blockquote answer-match.
///
/// The skill quotes the prompt it is answering as a blockquote (`> **Queue
/// prompt:**` / `> <text>`), so a free-text head is "answered" only when its text
/// appears in this quoted region -- prose that merely *mentions* a head (without
/// quoting it as a prompt) does NOT count, which keeps an unaddressed operator
/// report from being silently struck (#ftstrike false-strike guard).
///
/// `#qheadnest`: the match must be anchored to where a quoted entry *starts*,
/// not to "appears anywhere in the quoted region". One operator prompt commonly
/// quotes another head's text inside itself -- a bug report about the queue is
/// usually written by citing the queue line it is about. Searching one flattened
/// blob then reads the echo of prompt A as proof that head B was answered, and
/// the residue guard fails the closeout over a head nobody touched. Observed
/// 2026-08-08 on `agent-doc-bugs2.md`: an operator note whose body cited
/// ``tmux pane auto-sync is still extremely slow ...`` was echoed verbatim, and
/// that citation "answered" the still-queued auto-sync head.
///
/// Each candidate is a blockquote block joined from one of its lines to the end,
/// so a head spanning several quoted lines still matches at whatever entry
/// position it occupies, while text buried mid-entry never does.
fn response_blockquote_entry_candidates(response_body: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let mut block: Vec<String> = Vec::new();
    fn flush(block: &mut Vec<String>, candidates: &mut Vec<String>) {
        for start in 0..block.len() {
            let normalized = normalize_for_answer_match(&block[start..].join(" "));
            if !normalized.is_empty() {
                candidates.push(normalized);
            }
        }
        block.clear();
    }
    for line in response_body.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with('>') {
            flush(&mut block, &mut candidates);
            continue;
        }
        let quoted = trimmed.trim_start_matches('>').trim_start();
        let quoted = quoted
            .strip_prefix("**Queue prompt:**")
            .or_else(|| quoted.strip_prefix("**Queue prompts:**"))
            .unwrap_or(quoted)
            .trim_start();
        block.push(strip_echo_presence_list_marker(quoted).to_string());
    }
    flush(&mut block, &mut candidates);
    candidates
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
    strip_priority_markers(text).trim().to_string()
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

/// Queue bodies to search for a nest vector: the selected queue, then every
/// other `queue` component. A parse failure in one body skips that body.
fn queue_bodies_for_nest(content: &str) -> Vec<&str> {
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return Vec::new();
    };
    let mut bodies = Vec::new();
    if let Ok(Some(selected)) = crate::queue_set::selected_component(content, &components) {
        bodies.push(selected.content(content));
    }
    for component in components
        .iter()
        .filter(|component| component.name == "queue")
    {
        let body = component.content(content);
        if !bodies.contains(&body) {
            bodies.push(body);
        }
    }
    bodies
}

/// The first queue that contains `prompt_text`, with that row's index.
fn located_prompt(
    content: &str,
    prompt_text: &str,
) -> Option<(Vec<crate::document_queue::QueueEntry>, usize)> {
    for body in queue_bodies_for_nest(content) {
        let Ok(entries) = crate::document_queue::parse(body) else {
            continue;
        };
        if let Some(index) = crate::document_queue::prompt_index_for_text(&entries, prompt_text) {
            return Some((entries, index));
        }
    }
    None
}

/// Display nest vector of `prompt_text` (`#nestvector`), or `None` when the row
/// is top-level, folded into its parent (`#nestedsubprompts`), or absent.
pub fn nest_vector_in_document(content: &str, prompt_text: &str) -> Option<String> {
    let (entries, index) = located_prompt(content, prompt_text)?;
    crate::document_queue::nest_vector_at(&entries, index, prompt_text)
}

fn parsed_queue_bodies(content: &str) -> Vec<Vec<crate::document_queue::QueueEntry>> {
    queue_bodies_for_nest(content)
        .into_iter()
        .filter_map(|body| crate::document_queue::parse(body).ok())
        .collect()
}

fn first_submitted_in_document(
    content: &str,
    head_text: &str,
    skip: &impl Fn(&str) -> bool,
) -> Option<String> {
    for entries in parsed_queue_bodies(content) {
        if let Some(found) =
            crate::document_queue::locate_submitted_prompt(&entries, head_text, 0, skip)
        {
            return Some(found);
        }
    }
    None
}

/// Prompt text to submit for `head_text` (`#nestsubmit`).
///
/// A nested leaf becomes its nest vector. A top-level row, a folded
/// `#nestedsubprompts` head, a slash command, and text that is not a live row
/// stay `head_text` (slash and absent rows are unchanged; a top-level row is
/// its marker-stripped line when that differs). Completed rows do not match.
pub fn submitted_queue_prompt(content: &str, head_text: &str) -> String {
    submitted_queue_prompt_skipping(content, head_text, |_| false)
}

/// [`submitted_queue_prompt`], skipping live rows for which `skip` is true.
///
/// `skip` receives the row's raw text. Claim identity does not distinguish
/// duplicate leaves, so a caller that skips by identity skips every copy.
pub fn submitted_queue_prompt_skipping(
    content: &str,
    head_text: &str,
    skip: impl Fn(&str) -> bool,
) -> String {
    if head_text.trim().is_empty() || crate::queue_command::slash_command_text(head_text).is_some()
    {
        return head_text.to_string();
    }
    first_submitted_in_document(content, head_text, &skip).unwrap_or_else(|| head_text.to_string())
}

/// [`submitted_queue_prompt_skipping`] for a continuation diff.
///
/// Prefers the first matching live row that is also drainable. When that
/// filter matches nothing, falls back to the first unskipped match, then to
/// `head_text`.
pub fn submitted_continuation_prompt(
    content: &str,
    head_text: &str,
    skip: impl Fn(&str) -> bool,
) -> String {
    if head_text.trim().is_empty() || crate::queue_command::slash_command_text(head_text).is_some()
    {
        return head_text.to_string();
    }
    let prefer = |row: &str| {
        skip(row)
            || !crate::queue_continuation::is_drainable_queue_head(
                &agent_doc_document::queue_projection::strip_in_progress_marker(row),
            )
    };
    if let Some(found) = first_submitted_in_document(content, head_text, &prefer) {
        return found;
    }
    submitted_queue_prompt_skipping(content, head_text, skip)
}

/// Submitted prompts for `heads`, using the selected queue when it parses.
///
/// Heads are a subsequence in queue order. Each identical leaf takes the next
/// live row's parent (`#nestsubmit`).
pub fn submitted_prompts_for_heads(content: &str, heads: &[String]) -> Vec<String> {
    for entries in parsed_queue_bodies(content) {
        if entries
            .iter()
            .any(|entry| matches!(entry, crate::document_queue::QueueEntry::Prompt(_)))
        {
            return crate::document_queue::submitted_prompts_in_order(&entries, heads, |_| false);
        }
    }
    heads.to_vec()
}

/// True when the response carries an explicit `> **Queue prompt:**` echo of
/// this head's document-proven nest vector.
pub fn response_explicitly_echoes_nest_vector(
    content: &str,
    response_body: &str,
    head_text: &str,
) -> bool {
    nest_vector_in_document(content, head_text)
        .is_some_and(|vector| free_text_head_has_explicit_answer_evidence(response_body, &vector))
}

/// A descendant's nest-vector quote is that descendant's echo. It starts with
/// a long free-text ancestor, so the weak blockquote match would answer the
/// ancestor too. An exact echo of the ancestor itself still answers it.
fn descendant_nest_vector_blocks_ancestor(
    content: &str,
    response_body: &str,
    head_text: &str,
) -> bool {
    if response_explicit_queue_prompt_echoes_head(response_body, head_text) {
        return false;
    }
    let Some((entries, index)) = located_prompt(content, head_text) else {
        return false;
    };
    entries.iter().enumerate().any(|(leaf_idx, entry)| {
        if leaf_idx == index
            || !crate::document_queue::entry_is_ancestor_of(&entries, index, leaf_idx)
        {
            return false;
        }
        let leaf = match entry {
            crate::document_queue::QueueEntry::Prompt(prompt)
            | crate::document_queue::QueueEntry::Completed(prompt) => prompt,
            _ => return false,
        };
        crate::document_queue::nest_vector_at(&entries, leaf_idx, &leaf.text).is_some_and(
            |vector| response_explicit_queue_prompt_echoes_head(response_body, &vector),
        )
    })
}

fn echo_texts_for_embed(content: &str, consumed_texts: &[String]) -> Vec<String> {
    consumed_texts
        .iter()
        .map(|text| nest_vector_in_document(content, text).unwrap_or_else(|| text.clone()))
        .collect()
}

fn prompt_line_present_in_region(region: &str, prompt: &str) -> bool {
    let Some(first) = first_nonempty_line(prompt) else {
        return false;
    };
    let needle = normalize_prompt_echo_presence_line(first);
    !needle.is_empty()
        && region.lines().any(|line| {
            normalize_prompt_line(line) == needle
                || normalize_prompt_echo_presence_line(line) == needle
        })
}

fn echo_line_matches_prompt(line: &str, prompt_line: &str) -> bool {
    let needle = normalize_for_answer_match(&normalize_prompt_echo_presence_line(prompt_line));
    let got = normalize_for_answer_match(&normalize_prompt_echo_presence_line(line));
    !needle.is_empty() && got == needle
}

fn replace_echo_line_body(line: &str, new_body: &str) -> String {
    for label in ["**Queue prompt:**", "**Queue prompts:**"] {
        if let Some(idx) = line.find(label) {
            return format!("{} {}", &line[..idx + label.len()], new_body);
        }
    }
    if let Some(idx) = line.find('>') {
        return format!("{} {}", &line[..=idx], new_body);
    }
    line.to_string()
}

/// Rewrite a bare leaf echo (`> **Queue prompt:** item`) into its nest vector.
/// Lines inside fenced code stay untouched. The queue identity is not the echo.
fn rewrite_bare_nest_echoes(
    content: &str,
    consumed_texts: &[String],
    display_texts: &[String],
) -> String {
    let pairs: Vec<(&str, &str)> = consumed_texts
        .iter()
        .zip(display_texts.iter())
        .filter(|(identity, display)| identity.as_str() != display.as_str())
        .map(|(identity, display)| (identity.as_str(), display.as_str()))
        .collect();
    if pairs.is_empty() {
        return content.to_string();
    }
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return content.to_string();
    };
    let Some(exchange) = components
        .iter()
        .find(|component| component.name == "exchange")
    else {
        return content.to_string();
    };
    let code_ranges = agent_doc_element::element::find_code_ranges(content);
    let region = &content[exchange.open_end..exchange.close_start];
    let mut out = String::with_capacity(content.len() + 64);
    out.push_str(&content[..exchange.open_end]);
    let mut changed = false;
    let mut in_echo = false;
    let mut offset = exchange.open_end;
    for line in region.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        let in_code = code_ranges
            .iter()
            .any(|&(start, end)| line_start >= start && line_start < end);
        let trimmed = line.trim_start();
        let body = line.trim_end_matches(['\n', '\r']);
        if in_code || !trimmed.starts_with('>') {
            if !in_code && !trimmed.starts_with('>') && !trimmed.trim().is_empty() {
                in_echo = false;
            }
            out.push_str(line);
            continue;
        }
        let quoted = trimmed.trim_start_matches('>').trim_start();
        let opens_echo =
            quoted.starts_with("**Queue prompt:**") || quoted.starts_with("**Queue prompts:**");
        if opens_echo {
            in_echo = true;
        } else if !in_echo {
            out.push_str(line);
            continue;
        }
        let replacement = pairs.iter().find_map(|(identity, display)| {
            let id_first = first_nonempty_line(identity)?;
            let display_first = first_nonempty_line(display)?;
            echo_line_matches_prompt(body, id_first)
                .then(|| replace_echo_line_body(body, display_first))
        });
        if let Some(new_body) = replacement {
            changed = true;
            out.push_str(&new_body);
            if line.ends_with('\n') {
                out.push('\n');
            }
        } else {
            out.push_str(line);
        }
    }
    out.push_str(&content[exchange.close_start..]);
    if changed { out } else { content.to_string() }
}

/// Embed the consumed queue prompt echo immediately after this cycle's response
/// heading inside the `exchange` component. Returns `content` unchanged (fail-safe)
/// when the exchange/heading cannot be located, the prompt is empty, or the prompt
/// already appears in the exchange (e.g. a user typed it in directly).
///
/// A nested leaf (`#nestvector`) is echoed as its whole nest vector. A bare leaf
/// quote already in the exchange is rewritten to that vector and not inserted again.
pub fn embed_consumed_prompt_in_response(
    content: &str,
    consumed_texts: &[String],
    response_first_line: Option<&str>,
) -> String {
    if consumed_texts.iter().all(|t| t.trim().is_empty()) {
        return content.to_string();
    }
    let display_texts = echo_texts_for_embed(content, consumed_texts);
    let content = rewrite_bare_nest_echoes(content, consumed_texts, &display_texts);
    let Ok(components) = agent_doc_element::element::parse(&content) else {
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
    let max_chars = agent_doc_frontmatter::frontmatter::parse(&content)
        .ok()
        .and_then(|(fm, _)| fm.queue_prompt_echo_max_chars);
    let echo = format_consumed_prompt_echo(&display_texts, max_chars);
    if region.contains(echo.trim_end()) {
        return content.to_string();
    }
    let already_present = display_texts
        .iter()
        .any(|text| prompt_line_present_in_region(region, text));
    if already_present {
        return content.to_string();
    }

    let code_ranges = agent_doc_element::element::find_code_ranges(&content);
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
    let head_clean = strip_priority_markers(head_text);
    let head_norm = normalize_for_answer_match(&free_text_head_match_prose(&head_clean));
    if head_norm.is_empty() {
        return false;
    }

    // `#nestedechostrike`: a head with nested sub-prompts (`#nestedsubprompts`)
    // is one head whose text is the parent row plus its child rows, and its
    // verbatim echo spans one quoted line per row (`> **Queue prompt:** <parent>`
    // then `>   - <child>`). Matching each quoted line alone against the whole
    // multi-line head could never succeed, so a claimed nested head never
    // struck. The echo is matched as a contiguous run of quoted lines inside one
    // labeled echo: the run must start at a line boundary and cover the head
    // exactly, so a quote of the parent alone, or of one child, is not its echo.
    // A child that begins with `Include #subagent:` is release context, not a
    // sub-prompt. This matcher still recognizes that verbatim echo;
    // [`release_include_context_is_open`] refuses it while any named include
    // is a live queue row.
    let head_lines = head_echo_line_count(&head_clean);
    let mut run: Vec<String> = Vec::new();
    let mut in_queue_prompt_echo = false;
    for line in response_body.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with('>') {
            if !trimmed.trim().is_empty() {
                in_queue_prompt_echo = false;
                run.clear();
            }
            continue;
        }

        let quoted = trimmed.trim_start_matches('>').trim_start();
        let candidate = if let Some(rest) = quoted
            .strip_prefix("**Queue prompt:**")
            .or_else(|| quoted.strip_prefix("**Queue prompts:**"))
        {
            in_queue_prompt_echo = true;
            run.clear();
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
        run.push(normalize_for_answer_match(&candidate));
        if echo_run_ends_with_head(&run, &head_norm, head_lines) {
            return true;
        }
    }
    false
}

/// Number of non-empty normalized lines in a head's match prose: how many
/// quoted echo lines its verbatim echo spans (`#nestedechostrike`).
fn head_echo_line_count(head_clean: &str) -> usize {
    free_text_head_match_prose(head_clean)
        .lines()
        .filter(|line| !normalize_for_answer_match(line).is_empty())
        .count()
        .max(1)
}

/// True when the last `lines` non-empty entries of `run` (normalized quoted
/// echo lines, in order) join to exactly `head_norm` (`#nestedechostrike`).
/// A one-line head is the one-element suffix, so the single-line echo rule is
/// unchanged; a multi-line head needs one quoted line per head line.
fn echo_run_ends_with_head(run: &[String], head_norm: &str, lines: usize) -> bool {
    let tail: Vec<&str> = run
        .iter()
        .rev()
        .filter(|line| !line.is_empty())
        .take(lines)
        .map(String::as_str)
        .collect();
    if tail.len() != lines {
        return false;
    }
    let joined: Vec<&str> = tail.into_iter().rev().collect();
    joined.join(" ") == head_norm
}

const RELEASE_INCLUDE_PREFIX: &str = "Include #subagent:";
const SUBAGENT_LABEL_TOKEN: &str = "#subagent:";

/// Labels named by nested children that begin with `Include #subagent:`.
///
/// One child may name several labels (`Include #subagent:a + #subagent:b`).
/// Any other nested child is a sub-prompt, not release context.
fn release_include_subagent_labels(head_text: &str) -> Vec<String> {
    let mut labels = Vec::new();
    for row in agent_doc_element_queue::subprompts::subprompt_rows(head_text) {
        let work = include_row_work(&row);
        if !work.starts_with(RELEASE_INCLUDE_PREFIX) {
            continue;
        }
        for label in include_subagent_labels(work) {
            if !labels.iter().any(|existing| existing == &label) {
                labels.push(label);
            }
        }
    }
    labels
}

fn include_row_work(content: &str) -> &str {
    let work = agent_doc_element_queue::strip_attempt_projection_prefix(content).trim();
    let work = work.strip_prefix("~~").unwrap_or(work);
    match work.find("~~") {
        Some(end) => work[..end].trim(),
        None => work.trim(),
    }
}

fn include_subagent_labels(line: &str) -> Vec<String> {
    let mut labels = Vec::new();
    let mut rest = line;
    while let Some(index) = rest.find(SUBAGENT_LABEL_TOKEN) {
        let after = &rest[index + SUBAGENT_LABEL_TOKEN.len()..];
        let end = after
            .char_indices()
            .find(|(_, ch)| !ch.is_ascii_alphanumeric() && *ch != '-' && *ch != '_')
            .map(|(offset, _)| offset)
            .unwrap_or(after.len());
        if end == 0 {
            let skip = after.chars().next().map(char::len_utf8).unwrap_or(0);
            if skip == 0 {
                break;
            }
            rest = &after[skip..];
            continue;
        }
        let label = &after[..end];
        if !labels.iter().any(|existing| existing == label) {
            labels.push(label.to_string());
        }
        rest = &after[end..];
    }
    labels
}

/// True when a verbatim parent-plus-include echo must not strike (`#ftstrike`,
/// `#nestedechostrike`).
///
/// A nested child that begins with `Include #subagent:` names other subagent
/// work the parent must contain. That label is open while a different live
/// queue row carries the attempt owner `subagent:<label>` (for example
/// `🚧 [#subagent:nestedlineage#1]`). A struck or absent row is done. When
/// every named include is done, the echo may strike the parent and its include
/// lines together. A queue that cannot be read fails closed: the echo is not
/// an answer.
pub(crate) fn release_include_context_is_open(content: &str, head_text: &str) -> bool {
    let labels = release_include_subagent_labels(head_text);
    if labels.is_empty() {
        return false;
    }
    let Ok(nodes) = agent_doc_markdown_ast::mutations::item_nodes_all(content, "queue") else {
        return true;
    };
    labels.iter().any(|label| {
        let owner = format!("subagent:{label}");
        nodes.iter().any(|node| {
            if node.item.struck {
                return false;
            }
            let parent =
                agent_doc_element_queue::subprompts::head_without_subprompts(node.item.text.trim());
            agent_doc_document::queue_projection::owner_projection(parent)
                .is_some_and(|projection| projection.owner == owner)
        })
    })
}

/// True when a contiguous run of an echo block's normalized `entries` joins to
/// exactly `head_norm` (the multi-line head form of `entries.contains`).
fn echo_entries_contain_head(entries: &[String], head_norm: &str, lines: usize) -> bool {
    (1..=entries.len()).any(|end| echo_run_ends_with_head(&entries[..end], head_norm, lines))
}

/// One `> **Queue prompt:**` / `> **Queue prompts:**` echo block in a response.
struct QueuePromptEchoBlock {
    plural: bool,
    /// Normalized quoted entries (the prompt text, one per list item for plural).
    entries: Vec<String>,
    /// The next non-blank line after the block starts another echo block.
    followed_by_echo: bool,
    /// Nothing but blank lines follows the block.
    trailing: bool,
    /// One of the quoted entries is an id-backed directive (`do [#id]`,
    /// `[#id]`, a bare `#id`), whose completion goes through the id-aware reap
    /// path rather than through this echo.
    id_backed: bool,
}

fn queue_prompt_echo_blocks(response_body: &str) -> Vec<QueuePromptEchoBlock> {
    let lines: Vec<&str> = response_body.lines().collect();
    let echo_start = |line: &str| -> Option<(bool, String)> {
        let quoted = line.trim_start().strip_prefix('>')?.trim_start();
        if let Some(rest) = quoted.strip_prefix("**Queue prompts:**") {
            Some((true, rest.to_string()))
        } else {
            quoted
                .strip_prefix("**Queue prompt:**")
                .map(|rest| (false, rest.to_string()))
        }
    };
    let mut blocks = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let Some((plural, first)) = echo_start(lines[i]) else {
            i += 1;
            continue;
        };
        let mut raw = vec![first];
        i += 1;
        while i < lines.len() && lines[i].trim_start().starts_with('>') {
            if echo_start(lines[i]).is_some() {
                break;
            }
            raw.push(
                lines[i]
                    .trim_start()
                    .trim_start_matches('>')
                    .trim_start()
                    .to_string(),
            );
            i += 1;
        }
        let mut next = i;
        while next < lines.len() && lines[next].trim().is_empty() {
            next += 1;
        }
        let trailing = next >= lines.len();
        let followed_by_echo = !trailing && echo_start(lines[next]).is_some();
        let presence: Vec<String> = raw
            .iter()
            .map(|line| normalize_prompt_echo_presence_line(line))
            .filter(|line| !line.is_empty())
            .collect();
        let id_backed = presence.iter().any(|line| {
            let normalized = normalize_queue_prompt_text(line);
            crate::queue_directive::topic_resolves_to_only_id_directives(&normalized)
                .or_else(|| {
                    crate::queue_directive::explicit_do_directive_with_note_ids(&normalized)
                })
                .is_some_and(|ids| !ids.is_empty())
        });
        let entries = presence
            .iter()
            .map(|line| normalize_for_answer_match(line))
            .collect();
        blocks.push(QueuePromptEchoBlock {
            plural,
            entries,
            followed_by_echo,
            trailing,
            id_backed,
        });
    }
    blocks
}

/// `#bareechodrop`: a head quoted in a singular `> **Queue prompt:**` block
/// that is followed directly by another prompt's echo (or by nothing) was
/// listed, not answered. Observed on a Codex document: the response echoed
/// "Can we use https://fakecloud.dev/ to test our infrastructure locally?",
/// then immediately echoed `do [#sbxdeeplevers]` and answered only that — and
/// the bare echo alone struck the question from the queue, losing it. Grouping
/// several heads under one answer uses the plural `> **Queue prompts:**` list,
/// which this rule leaves alone.
///
/// `presetquotestrike` (live, agent-doc-bugs.md 2026-10-02): a run of
/// consecutive singular echoes followed by one combined answer is the same
/// grouping written one block per head. The response quoted four
/// `#gh-fix <url>` preset heads and one plain head, each in its own singular
/// block, then answered all five; only the LAST block (the one directly followed
/// by prose) struck, and the four preset heads stayed queued with a diagnostic
/// claiming they were never quoted. What made the fakecloud echo a listing was
/// that the run went on to quote an id-backed directive, whose answer belongs to
/// that directive. So a bare singular block is a listing only when the run of
/// echoes after it ends the response, or goes on to quote an id-backed head.
fn head_echoed_only_in_bare_singular_blocks(response_body: &str, head_clean: &str) -> bool {
    let head_norm = normalize_for_answer_match(&free_text_head_match_prose(head_clean));
    if head_norm.is_empty() {
        return false;
    }
    let blocks = queue_prompt_echo_blocks(response_body);
    let listed_only = |index: usize| -> bool {
        let block = &blocks[index];
        if block.plural {
            return false;
        }
        if block.trailing {
            return true;
        }
        if !block.followed_by_echo {
            return false;
        }
        let mut next = index + 1;
        while let Some(later) = blocks.get(next) {
            if later.id_backed || later.trailing {
                return true;
            }
            if !later.followed_by_echo {
                return false;
            }
            next += 1;
        }
        true
    };
    let matching: Vec<usize> = (0..blocks.len())
        .filter(|&index| {
            echo_entries_contain_head(
                &blocks[index].entries,
                &head_norm,
                head_echo_line_count(head_clean),
            )
        })
        .collect();
    !matching.is_empty() && matching.into_iter().all(listed_only)
}

/// The prose prefix of a free-text queue head used for answer-matching: every
/// line before the first fenced code block (` ``` ` or `~~~`). A head whose body
/// is dominated by a pasted console/route log (the common shape of an operator
/// bug report) is answered by quoting its prose lead, never the whole log, so
/// matching on the *entire* normalized node text (`#ftstrike-fence`) could never
/// strike it -- the response blockquote can't possibly `contains` the full log.
/// Matching on the prose prefix instead lets a code-fenced report strike when its
/// lead is quoted. Falls back to the whole text when there is no fence.
/// `#ftstrike-blockquote`: an embedded `>` blockquote line inside a queue head is
/// quoted prior-response context that the operator's directive sits above, not
/// part of the directive itself. It stops the prose prefix for the same reason a
/// fence does. Requiring the agent to reproduce that quoted context verbatim is
/// brittle in exactly the way the fence rule already anticipates: quoting the
/// operator's actual instruction failed to strike the head, so the drain re-ran
/// work that was already committed.
pub fn free_text_head_match_prose(head_text: &str) -> String {
    let mut prose = String::new();
    for line in head_text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") || trimmed.starts_with('>') {
            break;
        }
        prose.push_str(line);
        prose.push('\n');
    }
    prose
}

/// True when a `### Re:` topic repeats a distinctive code identifier from the
/// free-text head.
///
/// The ordinary free-text completion proof is the verbatim `> **Queue prompt:**`
/// echo. Historical sessions can predate that echo (or be closed by a harness
/// that omitted it), while still carrying an unambiguous response heading such
/// as ``### Re: Why `_load_identity_age_consent_embedded_runtime` was deleted``.
/// Limit this recovery path to long underscore-delimited identifiers: ordinary
/// prose, filenames, short symbols, and an identifier mentioned only in the
/// response body remain insufficient completion proof.
fn response_heading_targets_distinctive_identifier(response_body: &str, head_text: &str) -> bool {
    fn distinctive_identifiers(text: &str) -> Vec<String> {
        text.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
            .filter(|token| {
                token.len() >= 24
                    && token.bytes().filter(|byte| *byte == b'_').count() >= 2
                    && token.bytes().any(|byte| byte.is_ascii_alphabetic())
            })
            .map(str::to_ascii_lowercase)
            .collect()
    }

    let head_identifiers = distinctive_identifiers(head_text);
    if head_identifiers.is_empty() {
        return false;
    }
    let blocks: Vec<&str> = response_body.split("\n\n").collect();
    blocks.iter().enumerate().any(|(index, block)| {
        let targets_identifier = block
            .lines()
            .filter_map(response_heading_topic)
            .any(|topic| {
                let topic_identifiers = distinctive_identifiers(topic);
                head_identifiers
                    .iter()
                    .any(|identifier| topic_identifiers.contains(identifier))
            });
        if !targets_identifier {
            return false;
        }
        let mut scope = String::from(*block);
        if let Some(next) = blocks.get(index + 1) {
            scope.push(' ');
            scope.push_str(next);
        }
        !text_states_deferral(&scope)
    })
}

/// True when the current response explicitly echoes this queue prompt and has
/// answer prose after the echo. This is stronger than the broader matching
/// accepted by [`free_text_head_answered_by_response`]: callers may use it to
/// prove that the responder observed a queue head selected during the turn.
pub fn free_text_head_has_explicit_answer_evidence(response_body: &str, head_text: &str) -> bool {
    let response_body = direct_response_text(response_body);
    // Strip the leading operator/agent pin (`:pushpin:` ...) first -- its literal
    // shortcode word would otherwise survive normalization and break the match.
    let head_clean = strip_priority_markers(head_text);
    if head_echoed_only_in_bare_singular_blocks(&response_body, &head_clean) {
        return false;
    }
    response_explicit_queue_prompt_echoes_head(&response_body, &head_clean)
}

/// True when the committed `response_body` answers the free-text queue head
/// `head_text`: the head's normalized **prose prefix** (text before any fenced
/// code block -- see [`free_text_head_match_prose`]) appears inside the response's
/// quoted-prompt blockquote region. Requires a prose prefix of at least four
/// significant words so a short/empty head cannot match incidentally -- the
/// conservative direction, because a false positive silently drops an unaddressed
/// operator report.
pub fn free_text_head_answered_by_response(response_body: &str, head_text: &str) -> bool {
    let response_body = direct_response_text(response_body);
    // Strip the leading operator/agent pin (`:pushpin:` ...) first -- its literal
    // shortcode word would otherwise survive normalization and break the match.
    let head_clean = strip_priority_markers(head_text);
    if head_echoed_only_in_bare_singular_blocks(&response_body, &head_clean) {
        return false;
    }
    if free_text_head_has_explicit_answer_evidence(&response_body, &head_clean) {
        return true;
    }
    if response_heading_targets_distinctive_identifier(&response_body, &head_clean) {
        return true;
    }
    // `#ftstrike-fence`: match on the prose prefix, not the full node text -- a head
    // whose body is a pasted log is only ever quoted by its lead line(s).
    let head_prose = free_text_head_match_prose(&head_clean);
    let head_norm = normalize_for_answer_match(&head_prose);
    if head_norm.split(' ').filter(|w| !w.is_empty()).count() < 4 {
        return false;
    }
    // `#qheadnest`: anchored to a quoted entry's start, never "anywhere in the
    // quoted region" -- one prompt citing another head's text is not an answer
    // to that head.
    response_blockquote_entry_candidates(&response_body)
        .iter()
        .any(|entry| entry.starts_with(&head_norm))
}

/// Phrases with which a response states that a head it quoted was NOT actually
/// done this cycle (`#ftstrikedefer`).
///
/// Kept deliberately narrow and literal: each one is an explicit statement about
/// pending work, not a hedge.
/// The structured deferral marker (`#deferstrike`): a response that quotes a
/// free-text head and does NOT finish it this cycle opens the paragraph right
/// after the `> **Queue prompt:**` quote with this bold lead (any bold
/// `**Deferred…**` lead counts, e.g. `**Deferred, not done:**`). The quote then
/// satisfies the pre-write evidence gate while the head stays queued.
pub const QUEUE_PROMPT_DEFERRAL_MARKER: &str = "**Deferred:**";

/// Lowercased structured deferral lead (`QUEUE_PROMPT_DEFERRAL_MARKER` and its
/// bold variants such as `**Deferred, not done:**`).
const STRUCTURED_DEFERRAL_MARKER: &str = "**deferred";

const FREE_TEXT_HEAD_DEFERRAL_MARKERS: &[&str] = &[
    // Structured lead (`QUEUE_PROMPT_DEFERRAL_MARKER` and its bold variants).
    "**deferred",
    // Natural phrasing a coordinator uses for claimed/dispatched work
    // (`#deferstrike`, agent-doc-bugs.md 2026-10-04: `release + publish` was
    // struck while "Deferred, not done ... Keep this head open").
    "deferred, not done",
    "deferred until",
    "not done yet",
    "not yet done",
    "is not done",
    "has not happened yet",
    "keep this head open",
    "keep this head queued",
    "keep the head open",
    "keep the head queued",
    "dispatched to subagent",
    "dispatched to a subagent",
    "dispatched to its own subagent",
    "dispatched to the subagent",
    "waits for",
    "waiting on",
    "queued and untouched",
    "still queued",
    "remains queued",
    "left queued",
    "not been started",
    "have not started",
    "has not started",
    "next cycle will",
    "the next cycle",
    "a later cycle",
    "deferred to the next",
    "left for the next",
    "will be handled next",
];

/// True when the response both quotes `head_text` AND states, next to that quote,
/// that the head is still outstanding (`#ftstrikedefer`).
///
/// `#ftstrike` treats a quoted-prompt echo as proof that the cycle answered the
/// head. That is only an assertion by the responding agent — nothing verifies the
/// work happened. A response that quotes a head and in the same breath calls it
/// "queued and untouched" is explicitly saying it did NOT do the work, yet the
/// echo alone still struck it, silently marking unrun work complete.
///
/// The scan is scoped to the blockquote block that carries the quote plus the
/// block immediately after it, so an unrelated deferral elsewhere in a long
/// response does not hold back a genuinely answered head. Returning `true` only
/// ever PREVENTS a strike, so a false positive costs one extra cycle while a
/// false negative loses the operator's work.
pub fn response_defers_free_text_head(response_body: &str, head_text: &str) -> bool {
    let response_body = direct_response_text(response_body);
    let head_clean = strip_priority_markers(head_text);
    let head_norm = normalize_for_answer_match(&free_text_head_match_prose(&head_clean));
    if head_norm.is_empty() {
        return false;
    }
    let weak_quote_allowed = head_norm.split(' ').filter(|word| !word.is_empty()).count() >= 4;
    let blocks: Vec<&str> = response_body.split("\n\n").collect();
    for (index, block) in blocks.iter().enumerate() {
        let exact_labeled_echo = response_explicit_queue_prompt_echoes_head(block, &head_clean);
        let anchored_weak_quote = weak_quote_allowed
            && response_blockquote_entry_candidates(block)
                .iter()
                .any(|entry| entry.starts_with(&head_norm));
        if !exact_labeled_echo && !anchored_weak_quote {
            continue;
        }
        if deferral_scope_has_marker(&blocks, index) {
            return true;
        }
    }
    false
}

/// True when the LAST `> **Queue prompt:**`-style blockquote echo of `head_text`
/// in `text` (normally the committed exchange) is followed by a deferral
/// (`#deferstrike`).
///
/// The exchange accumulates every cycle's responses, so the most recent echo is
/// the current statement about the head: a deferral there keeps the head out of
/// the completed-residue set, while a later answered echo (the closing cycle)
/// makes it residue again.
pub fn latest_free_text_head_echo_is_deferral(text: &str, head_text: &str) -> bool {
    let text = direct_response_text(text);
    let head_clean = strip_priority_markers(head_text);
    let head_norm = normalize_for_answer_match(&free_text_head_match_prose(&head_clean));
    if head_norm.is_empty() {
        return false;
    }
    let weak_quote_allowed = head_norm.split(' ').filter(|word| !word.is_empty()).count() >= 4;
    let blocks: Vec<&str> = text.split("\n\n").collect();
    let Some(index) = blocks.iter().rposition(|block| {
        response_explicit_queue_prompt_echoes_head(block, &head_clean)
            || (weak_quote_allowed
                && response_blockquote_entry_candidates(block)
                    .iter()
                    .any(|entry| entry.starts_with(&head_norm)))
    }) else {
        return false;
    };
    deferral_scope_has_marker(&blocks, index)
}

/// [`response_defers_free_text_head`] for a nested leaf quoted as its nest vector.
///
/// A vector quote is that leaf's echo. Deferral beside it keeps the leaf queued.
/// The bare leaf line remains a deferral too.
pub fn response_defers_queue_head(content: &str, response_body: &str, head_text: &str) -> bool {
    if response_defers_free_text_head(response_body, head_text) {
        return true;
    }
    nest_vector_in_document(content, head_text)
        .is_some_and(|vector| response_defers_free_text_head(response_body, &vector))
}

/// [`latest_free_text_head_echo_is_deferral`] across the bare head and its nest vector.
///
/// The later of the two echoes is the current statement. An older deferral does
/// not outrank a later answer, and a later vector deferral keeps the leaf queued.
pub fn latest_queue_head_echo_is_deferral(content: &str, text: &str, head_text: &str) -> bool {
    let text = direct_response_text(text);
    let mut heads = vec![head_text.to_string()];
    if let Some(vector) = nest_vector_in_document(content, head_text)
        && nest_row_work_differs(&vector, head_text)
    {
        heads.push(vector);
    }
    let blocks: Vec<&str> = text.split("\n\n").collect();
    let Some(index) = blocks.iter().rposition(|block| {
        heads
            .iter()
            .any(|head| block_quotes_free_text_head(block, head))
    }) else {
        return false;
    };
    deferral_scope_has_marker(&blocks, index)
}

fn nest_row_work_differs(vector: &str, head_text: &str) -> bool {
    vector.trim() != strip_priority_markers(head_text).trim()
}

fn block_quotes_free_text_head(block: &str, head_text: &str) -> bool {
    let head_clean = strip_priority_markers(head_text);
    let head_norm = normalize_for_answer_match(&free_text_head_match_prose(&head_clean));
    if head_norm.is_empty() {
        return false;
    }
    let weak_quote_allowed = head_norm.split(' ').filter(|word| !word.is_empty()).count() >= 4;
    response_explicit_queue_prompt_echoes_head(block, &head_clean)
        || (weak_quote_allowed
            && response_blockquote_entry_candidates(block)
                .iter()
                .any(|entry| entry.starts_with(&head_norm)))
}

/// The part of `current_exchange` written after `anchor_exchange`
/// (`#qheadresiduescope`).
///
/// `anchor_exchange` is the exchange as it stood when a queue head (re-)entered
/// the queue. Responses land at one end of the exchange (`patch=append`, or
/// `prepend`), so the newer text is what lies between the longest common
/// leading run of lines and the longest common trailing run. The two runs never
/// overlap the anchor, so when the anchor is a whole prefix (append) the result
/// is exactly the appended tail. A compaction or edit of older history shrinks
/// the common runs, which only widens the result back toward the whole
/// exchange: the scoping can hide an old echo, never a new one.
pub fn exchange_text_newer_than(current_exchange: &str, anchor_exchange: &str) -> String {
    let current: Vec<&str> = current_exchange.lines().collect();
    let anchor: Vec<&str> = anchor_exchange.lines().collect();
    let max_common = current.len().min(anchor.len());
    let prefix = current
        .iter()
        .zip(anchor.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let suffix = current
        .iter()
        .rev()
        .zip(anchor.iter().rev())
        .take(max_common - prefix)
        .take_while(|(a, b)| a == b)
        .count();
    current[prefix..current.len() - suffix].join("\n")
}

/// `text` without its Markdown code (inline spans and blocks), whitespace
/// collapsed and lowercased, so a deferral phrase wrapped across lines still
/// matches and a quoted one does not.
///
/// `#deferquotedmarker`: a code span or block only quotes a phrase. A response
/// explaining the strike rule (``with no `**Deferred:**` line beside it``) is
/// not deferring the head it just answered.
fn deferral_scan_text(text: &str) -> String {
    let ranges = agent_doc_element::element::find_code_ranges(text);
    let mut prose = String::with_capacity(text.len());
    let mut cursor = 0;
    for (start, end) in ranges {
        if start < cursor || end > text.len() {
            continue;
        }
        prose.push_str(&text[cursor..start]);
        prose.push(' ');
        cursor = end;
    }
    prose.push_str(&text[cursor..]);
    prose
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

/// True when `text` states a deferral (`#ftstrikedefer`): a
/// [`FREE_TEXT_HEAD_DEFERRAL_MARKERS`] phrase outside Markdown code.
fn text_states_deferral(text: &str) -> bool {
    let prose = deferral_scan_text(text);
    FREE_TEXT_HEAD_DEFERRAL_MARKERS
        .iter()
        .any(|marker| prose.contains(marker))
}

/// The lead statement of a block: its first list item (with continuation
/// lines) when the block is a list, otherwise the whole paragraph.
fn block_lead_statement(block: &str) -> String {
    let is_item = |line: &str| {
        let line = line.trim_start();
        line.starts_with("- ")
            || line.starts_with("* ")
            || line.starts_with("+ ")
            || line
                .split_once(". ")
                .is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
    };
    let mut lines = block.lines().skip_while(|line| line.trim().is_empty());
    let Some(first) = lines.next() else {
        return String::new();
    };
    if !is_item(first) {
        return block.to_string();
    }
    let indent = first.len() - first.trim_start().len();
    let mut lead = vec![first];
    for line in lines {
        let line_indent = line.len() - line.trim_start().len();
        if line_indent <= indent && is_item(line) {
            break;
        }
        lead.push(line);
    }
    lead.join("\n")
}

/// Whether the block at `index` plus the block after it states a deferral.
///
/// Blockquote lines are the head's own echo, not the response's statement
/// about it, so they are left out: a head whose text happens to contain a
/// marker (`answered but still queued`) must not defer itself.
///
/// `#annotateddeferstrike`: an annotated head must also be answered with a
/// `> **Operator note:** <annotation>` quote (`#qheadannotation`), which the
/// canonical response places between the `> **Queue prompt:**` echo and the
/// deferral paragraph. Those operator-note blocks belong to the echo, so they
/// are skipped and the block after them is the one that states the deferral.
///
/// `#deferquotedmarker`: markers inside Markdown code are quotations, never a
/// deferral. In the block after the echo, the structured `**Deferred…**` lead
/// counts anywhere, but a natural-language phrase (`waiting on`, `the next
/// cycle`, ...) counts only in the block's lead statement: its first list item,
/// or the whole paragraph when it is not a list. A later bullet in an answer
/// usually describes other work. Live 2026-10-28 (agent-doc-bugs.md): the
/// answer to the reported `I'm still seeing run items in a ☑️ state …` row
/// quoted it exactly, explained the rule as ``no `**Deferred:**` line``, and
/// said in its sixth bullet that three other rows were "waiting on your
/// choices". Both read as a deferral, so that row alone survived a closeout
/// that struck its two quoted siblings.
fn deferral_scope_has_marker(blocks: &[&str], index: usize) -> bool {
    let prose = |block: &str| {
        block
            .lines()
            .filter(|line| !line.trim_start().starts_with('>'))
            .collect::<Vec<_>>()
            .join("\n")
    };
    if text_states_deferral(&prose(blocks[index])) {
        return true;
    }
    let Some(next) = blocks
        .iter()
        .skip(index + 1)
        .find(|block| !block_is_operator_note_quote(block))
    else {
        return false;
    };
    let next = prose(next);
    deferral_scan_text(&next).contains(STRUCTURED_DEFERRAL_MARKER)
        || text_states_deferral(&block_lead_statement(&next))
}

/// True when `block` is nothing but a `> **Operator note:** ...` blockquote
/// (`#qheadannotation` answer evidence), optionally wrapped over several
/// quoted lines.
fn block_is_operator_note_quote(block: &str) -> bool {
    let mut lines = block.lines().map(str::trim).filter(|line| !line.is_empty());
    let Some(first) = lines.next() else {
        return false;
    };
    first.strip_prefix('>').is_some_and(|quoted| {
        quoted
            .trim_start()
            .starts_with(crate::queue_head_annotation::OPERATOR_NOTE_LABEL)
    }) && lines.all(|line| line.starts_with('>'))
}

/// True when `head_text`'s normalized prose prefix matches a free-text queue head
/// present in the stable pre-turn `baseline` document.
///
/// This is the conservative same-cycle strike gate: a head that first appeared in
/// the live buffer during the current turn is treated as not present, so callers
/// can defer striking it until a later cycle. The match reuses the free-text
/// answer normalization and therefore ignores pin/list cosmetics. A head with
/// fewer than four significant prose words is too ambiguous and returns false.
pub fn free_text_head_present_in_baseline(baseline: &str, head_text: &str) -> bool {
    let head_clean = strip_priority_markers(head_text);
    let head_norm = normalize_for_answer_match(&free_text_head_match_prose(&head_clean));
    if head_norm.is_empty() {
        return false;
    }
    let Ok(nodes) = agent_doc_markdown_ast::mutations::item_nodes_all(baseline, "queue") else {
        return false;
    };
    nodes.iter().any(|node| {
        let base_clean = strip_priority_markers(node.item.text.trim());
        let base_norm = normalize_for_answer_match(&free_text_head_match_prose(&base_clean));
        !base_norm.is_empty() && base_norm == head_norm
    })
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
    fn bare_singular_echo_followed_by_another_echo_is_not_an_answer() {
        let head = "Can we use https://fakecloud.dev/ to test our infrastructure locally?";
        let dropped = concat!(
            "### Re: The deep sandbox levers are capability decisions — codex\n\n",
            "> **Queue prompt:**\n",
            ">\n",
            "> Can we use https://fakecloud.dev/ to test our infrastructure locally?\n\n\n",
            "> **Queue prompt:**\n",
            ">\n",
            "> do [#sbxdeeplevers]\n\n",
            "I do not recommend changing either lever without a product decision.\n",
        );
        assert!(!free_text_head_answered_by_response(dropped, head));

        // The same echo followed by its own answer still counts.
        let answered = concat!(
            "### Re: fakecloud — codex\n\n",
            "> **Queue prompt:** Can we use https://fakecloud.dev/ to test our infrastructure locally?\n\n",
            "Partly: it emulates the AWS APIs we use but not ECS task networking.\n",
        );
        assert!(free_text_head_answered_by_response(answered, head));

        // An echo as the very last thing in a response answered nothing.
        let trailing = "### Re: x — codex\n\nDid other work.\n\n> **Queue prompt:** Can we use https://fakecloud.dev/ to test our infrastructure locally?\n";
        assert!(!free_text_head_answered_by_response(trailing, head));

        // Grouping uses the plural list form and is left alone.
        let grouped = concat!(
            "### Re: two — codex\n\n",
            "> **Queue prompts:**\n",
            "> - Can we use https://fakecloud.dev/ to test our infrastructure locally?\n",
            "> - Should the sandbox keep Temporal running?\n\n",
            "Both are capability decisions; answered together below.\n",
        );
        assert!(free_text_head_answered_by_response(grouped, head));
    }

    /// `presetquotestrike`: consecutive singular echoes followed by one combined
    /// answer group their heads the same way the plural list does.
    #[test]
    fn consecutive_singular_echoes_followed_by_an_answer_answer_every_head() {
        let response = concat!(
            "### Re: parallel drain — opus-5.5\n\n",
            "> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/80\n\n",
            "> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/87\n\n",
            "> **Queue prompt:** The Idea Plugin formatting is not active. It's gone now.\n\n",
            "All of it shipped.\n",
        );
        for head in [
            "#gh-fix https://github.com/btakita/agent-doc/issues/80",
            "#gh-fix https://github.com/btakita/agent-doc/issues/87",
            "The Idea Plugin formatting is not active. It's gone now.",
        ] {
            assert!(
                free_text_head_answered_by_response(response, head),
                "{head} was quoted and answered"
            );
        }

        // A run of echoes that ends the response answered nothing.
        let trailing = concat!(
            "### Re: x — opus-5.5\n\nDid other work.\n\n",
            "> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/80\n\n",
            "> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/87\n",
        );
        assert!(!free_text_head_answered_by_response(
            trailing,
            "#gh-fix https://github.com/btakita/agent-doc/issues/80"
        ));
    }

    #[test]
    fn free_text_head_present_in_baseline_ignores_pin_and_dash_cosmetics() {
        let baseline = concat!(
            "<!-- agent:queue -->\n",
            "- My free-text queue items are not immediately struck as if they are addressed.\n",
            "<!-- /agent:queue -->\n",
        );
        assert!(free_text_head_present_in_baseline(
            baseline,
            ":pushpin: My free-text queue items are not immediately struck as if they are addressed."
        ));
        assert!(!free_text_head_present_in_baseline(
            baseline,
            "An unrelated head about a completely separate matter entirely"
        ));
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

    /// `#claimrowtombstone`: the binary's own attempt annotation is never the
    /// row's id. Observed 2026-10-26: `☑️ [#subagent:claimrowmigrate#1] do
    /// [#claimrowmigrate]` read as `subagent`, which tombstoned
    /// `claimrowmigrate` as operator-deleted and let convergence drop the row.
    #[test]
    fn queue_prompt_done_id_ignores_lifecycle_and_owner_projection() {
        for row in [
            "☑️ [#subagent:claimrowmigrate#1] do [#claimrowmigrate]",
            "🚧 [#subagent:claimrowmigrate#12] do [#claimrowmigrate]",
            "🟡 ⟦subagent:claimrowmigrate#1⟧ do [#claimrowmigrate]",
            "📥 do [#claimrowmigrate]",
        ] {
            assert_eq!(
                queue_prompt_done_id(row),
                Some("claimrowmigrate".to_string()),
                "{row}"
            );
        }
        assert_eq!(
            queue_prompt_done_id("☑️ [#subagent:release_publish_patch#3] release + publish"),
            None,
            "a free-text row carries no id once its attempt annotation is ignored"
        );
    }

    /// `#taggedidhead`: a leading intent tag never becomes the done id of an
    /// id-backed row; the work identity's id does.
    #[test]
    fn queue_prompt_done_id_skips_leading_intent_tags_of_id_backed_rows() {
        for row in [
            "#review [#adr0043-publish]",
            "☑️ [#subagent:adr0043-review#1] #review [#adr0043-publish]",
            "#subagents do [#adr0043-publish]",
        ] {
            assert_eq!(
                queue_prompt_done_id(row),
                Some("adr0043-publish".to_string()),
                "{row}"
            );
            assert_eq!(
                agent_doc_element_queue::queue_head_identity(row),
                agent_doc_element_queue::QueueItemIdentity::Id("adr0043-publish".into()),
                "the done id and the claim identity are one key: {row}"
            );
        }
        // A free-text identity keeps the first-`#` reading.
        assert_eq!(
            queue_prompt_done_id("#fix1 do the thing"),
            Some("fix1".to_string())
        );
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
        assert!(queue_prompt_text_is_free_text(content, "#orphanqhead"));
        assert!(!queue_prompt_text_is_free_text(content, "do queue"));
        assert!(queue_prompt_text_is_free_text(
            content,
            "My free-text queue items are not immediately struck as if they are addressed."
        ));
        assert!(queue_prompt_text_is_free_text(
            content,
            "Approve [#shoptiers] then ship it"
        ));
        // `#annotateddirective`: an operator note after the directive keeps it
        // id-backed, so only `--done editorauth2` may complete it.
        assert!(!queue_prompt_text_is_free_text(
            content,
            "do [#editorauth2]: ensure plugin parity"
        ));
        assert!(queue_prompt_text_is_free_text(
            content,
            "[#editorauth2]: a note"
        ));
    }

    #[test]
    fn head_leading_with_an_open_backlog_id_is_never_free_text() {
        // GH #129: `[#id] [operator-verify] prose` is the shape the
        // `#queue-clear-unrun-items` guard reported as silently dropped after the
        // answered-free-text strike completed it with its backlog item open.
        let content = concat!(
            "---\nqueue_active: true\n---\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#confirmwhethersoftwrap] [operator-verify] Confirm whether the softwrap-caret fix works\n",
            "<!-- /agent:backlog -->\n",
            "<!-- agent:queue -->\n- x\n<!-- /agent:queue -->\n",
        );
        for head in [
            "[#confirmwhethersoftwrap] [operator-verify] Confirm whether the softwrap-caret fix works",
            ":pushpin: [#confirmwhethersoftwrap] [operator-verify] Confirm whether it works",
            "do [#confirmwhethersoftwrap] and confirm the terminal cursor symptom",
            "#confirmwhethersoftwrap confirm the terminal cursor symptom",
        ] {
            assert!(
                !queue_prompt_text_is_free_text(content, head),
                "head {head:?} leads with an open backlog id, so it is id-backed"
            );
        }
        // A head that only starts with a resolved/unknown id stays free text,
        // and so does prose that merely mentions the open id later.
        assert!(queue_prompt_text_is_free_text(
            content,
            "[#resolvedlongago] follow-up question about the old fix"
        ));
        assert!(queue_prompt_text_is_free_text(
            content,
            "Approve [#confirmwhethersoftwrap] then ship it"
        ));
        // The inert `[#id]: note` shape keeps its contract.
        assert!(queue_prompt_text_is_free_text(
            content,
            "[#confirmwhethersoftwrap]: a note"
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
            queue_head_is_free_text_prompt(&active_queue_doc("[#todo-a]: note")).unwrap(),
            "a bare bracketed id followed by a colon is an inert free-text note"
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
        assert!(active_queue_head_is_registered_preset(registered, "advance-review").unwrap());

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
        assert!(
            !active_queue_head_is_registered_preset(preset_and_tracked, "advance-review").unwrap()
        );

        assert!(queue_head_is_free_text_prompt(&active_queue_doc("#advance-review")).unwrap());
        assert!(
            !active_queue_head_is_registered_preset(
                &active_queue_doc("#advance-review"),
                "advance-review"
            )
            .unwrap()
        );

        let unknown_but_open = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- #advance-review\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#advance-review] tracked directive\n",
            "<!-- /agent:backlog -->\n",
        );
        assert!(!queue_head_is_free_text_prompt(unknown_but_open).unwrap());
    }

    #[test]
    fn prompt_preset_response_evidence_uses_expansions_for_composable_heads() {
        let content = concat!(
            "---\nqueue_active: true\n",
            "prompt_presets:\n",
            "  '#upgrade': Upgrade agent-doc and verify the current issues.\n",
            "  '#gh-fix': Fix the referenced GitHub issue and add a regression test.\n",
            "---\n\n",
            "<!-- agent:queue auto -->\n",
            "- [#upgrade]\n",
            "<!-- /agent:queue -->\n",
        );
        let response = concat!(
            "### Re: upgrade\n\n",
            "Upgrade agent-doc and verify the current issues. ",
            "Fix the referenced GitHub issue and add a regression test. ",
            "Fixed https://github.com/example/sample/issues/100."
        );

        assert!(prompt_preset_head_answered_by_response(
            content,
            response,
            "[#upgrade]"
        ));
        assert!(prompt_preset_head_answered_by_response(
            content,
            response,
            "#gh-fix https://github.com/example/sample/issues/100"
        ));
        assert!(prompt_preset_head_answered_by_response(
            content,
            response,
            "#upgrade #gh-fix https://github.com/example/sample/issues/100"
        ));
        assert!(!prompt_preset_head_answered_by_response(
            content,
            "### Re: partial\n\nUpgrade agent-doc and verify the current issues.",
            "#upgrade #gh-fix https://github.com/example/sample/issues/100"
        ));
    }

    #[test]
    fn prompt_preset_expansions_ignore_inline_code_and_follow_item_order() {
        let content = concat!(
            "---\nprompt_presets:\n",
            "  '#upgrade': Upgrade agent-doc.\n",
            "  '#gh-issue': File a GitHub issue.\n",
            "---\n",
        );

        assert_eq!(
            queue_prompt_preset_expansions(content, "#gh-issue: do not run `#upgrade` here",),
            vec![("#gh-issue".to_string(), "File a GitHub issue.".to_string())]
        );
        assert_eq!(
            queue_prompt_preset_expansions(
                content,
                "#gh-issue: quoted row `- ~~#upgrade~~ — auto-struck`",
            ),
            vec![("#gh-issue".to_string(), "File a GitHub issue.".to_string())]
        );
        assert_eq!(
            queue_prompt_preset_expansions(
                content,
                "#gh-issue then #upgrade, then #gh-issue again",
            ),
            vec![
                ("#gh-issue".to_string(), "File a GitHub issue.".to_string()),
                ("#upgrade".to_string(), "Upgrade agent-doc.".to_string()),
            ]
        );
    }

    /// `#ftstrikeclaimedmention` (agent-doc-bugs.md 2026-10-09): a status
    /// response quoting the `#gh-fix` preset body once must not answer every
    /// `#gh-fix <url>` head. The head's own argument (its URL) must appear, on
    /// whole-word boundaries.
    #[test]
    fn prompt_preset_body_mention_does_not_answer_every_preset_head() {
        let content = concat!(
            "---\nqueue_active: true\n",
            "prompt_presets:\n",
            "  '#gh-fix': fix then close\n",
            "---\n\n",
            "<!-- agent:queue go -->\n",
            "- #gh-fix https://github.com/btakita/agent-doc/issues/218\n",
            "- #gh-fix https://github.com/btakita/agent-doc/issues/22\n",
            "- #gh-fix https://github.com/btakita/agent-doc/issues/227\n",
            "<!-- /agent:queue -->\n",
        );
        let status = concat!(
            "### Re: GH #227 and #228 dispatched\n\n",
            "Both new `#gh-fix` heads (\"fix then close\") are claimed and with their own ",
            "subagents. GH #218 waits on PR #222.\n",
        );
        for head in [
            "#gh-fix https://github.com/btakita/agent-doc/issues/218",
            "#gh-fix https://github.com/btakita/agent-doc/issues/22",
            "#gh-fix https://github.com/btakita/agent-doc/issues/227",
        ] {
            assert!(
                !prompt_preset_head_answered_by_response(content, status, head),
                "a preset-body mention is not an answer to {head}"
            );
        }
        let answered = concat!(
            "### Re: GH 227\n\n",
            "fix then close: fixed https://github.com/btakita/agent-doc/issues/227 and closed it.\n",
        );
        assert!(prompt_preset_head_answered_by_response(
            content,
            answered,
            "#gh-fix https://github.com/btakita/agent-doc/issues/227"
        ));
        assert!(
            !prompt_preset_head_answered_by_response(
                content,
                answered,
                "#gh-fix https://github.com/btakita/agent-doc/issues/22"
            ),
            "`.../issues/227` must not prove `.../issues/22`"
        );
        assert_eq!(preset_head_argument("[#upgrade]"), "");
        assert_eq!(
            preset_head_argument("🚧 #subagents #gh-fix https://x.dev/issues/9"),
            "https x dev issues 9"
        );
    }

    /// `#ftstrikeclaimedmention`: a claimed head accepts only an exact,
    /// agent-authored `> **Queue prompt:**` echo — never a mention, a preset
    /// body, a heading, or a blockquote that merely begins with its prose.
    #[test]
    fn claimed_head_proof_is_exact_queue_prompt_echo_only() {
        let content = "---\nprompt_presets:\n  '#gh-fix': fix then close\n---\n";
        let head = "#gh-fix https://github.com/btakita/agent-doc/issues/221";
        let claimed = |response: &str| {
            queue_head_answered_by_response_with_claim(content, response, head, true)
        };
        let mention = "### Re: status\n\nfix then close: https://github.com/btakita/agent-doc/issues/221 waits on PR 222.\n";
        assert!(!claimed(mention));
        assert!(
            queue_head_answered_by_response_with_claim(content, mention, head, false),
            "the same text is preset evidence for an UNclaimed head"
        );
        let quoted_prefix = concat!(
            "### Re: x\n\n",
            "> #gh-fix https://github.com/btakita/agent-doc/issues/221 and more prose\n\n",
            "Done.\n",
        );
        assert!(!claimed(quoted_prefix));
        let other_head_echo = concat!(
            "### Re: x\n\n",
            "> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/218\n\n",
            "Done.\n",
        );
        assert!(!claimed(other_head_echo));
        let exact = concat!(
            "### Re: GH 221\n\n",
            "> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/221\n\n",
            "Merged PR 222 and closed GH 221.\n",
        );
        assert!(claimed(exact));
        let exact_multiline = concat!(
            "### Re: GH 221\n\n",
            "> **Queue prompt:**\n>\n> #gh-fix https://github.com/btakita/agent-doc/issues/221\n\n",
            "Merged PR 222 and closed GH 221.\n",
        );
        assert!(claimed(exact_multiline));
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
    fn response_explicitly_targets_queue_head_matches_heading_topic_or_exact_id() {
        assert!(response_explicitly_targets_queue_head(
            "### Re: do [#foo]\n\nDone.",
            "do [#foo]"
        ));
        assert!(response_explicitly_targets_queue_head(
            "### Re: #foo — gpt-5\n\nDone.",
            "do [#foo]"
        ));
        assert!(!response_explicitly_targets_queue_head(
            "### Re: #foo deferred\n\nBlocked.",
            "do [#foo]"
        ));
        assert!(!response_explicitly_targets_queue_head(
            "No response heading here, just prose #foo.",
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
    fn free_text_head_answered_by_response_ignores_a_head_cited_inside_another_prompt() {
        // #qheadnest FALSE-STRIKE GUARD: an operator bug report about the queue
        // is normally written by citing the queue line it is about. Echoing that
        // report must not count as answering the head it cites -- flattening every
        // blockquote line into one blob did exactly that, and the residue guard
        // then INTERRUPTED the closeout over a head nobody touched.
        let cited = concat!(
            "tmux pane auto-sync is still extremely slow when switching from ",
            "lazily.md to agent-doc-bugs2.md. Fix it."
        );
        let response = format!(
            concat!(
                "### Re: do [#runsubmitclaude] -- opus-5\n\n",
                "> **Queue prompt:**\n",
                ">\n",
                "> do [#runsubmitclaude]\n",
                ">\n",
                "> The free-text `{cited}` was kept when you added ",
                "`do [#runsubmitclaude]` into the queue. The free-text should have ",
                "been replaced (deleted) by `do [#runsubmitclaude]`.\n\n",
                "Fixed and released.\n",
            ),
            cited = cited
        );
        assert!(
            !free_text_head_answered_by_response(&response, cited),
            "a head quoted inside another prompt's body is cited, not answered"
        );

        // The citing prompt itself IS answered by that same echo.
        let citing = format!(
            "The free-text `{cited}` was kept when you added `do [#runsubmitclaude]` into the queue."
        );
        assert!(
            free_text_head_answered_by_response(&response, &citing),
            "the echoed entry still answers its own head"
        );
    }

    #[test]
    fn free_text_head_answered_by_response_matches_a_multi_line_head_at_any_entry_position() {
        // The anchor is the start of a quoted entry, not the start of the block,
        // so a head spanning two quoted lines still matches in second position.
        let head = "the sync pane collapses to one column\nafter the second navigation";
        let response = concat!(
            "### Re: two reports -- opus\n\n",
            "> **Queue prompts:**\n",
            "> - deploy the release build\n",
            "> - the sync pane collapses to one column\n",
            "> after the second navigation\n\n",
            "Both handled.\n",
        );
        assert!(free_text_head_answered_by_response(response, head));
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
    fn free_text_head_answered_by_distinctive_identifier_response_heading() {
        let head = concat!(
            "Please help me understand why you deleted ",
            "_load_identity_age_consent_embedded_runtime in ",
            "https://github.com/haiven-dev/fpe/pull/12/changes#diff-example. ",
            "Was this due to \"remove the duplicate identity embedded-runtime implementation\"?"
        );
        let response = concat!(
            "### Re: Why `_load_identity_age_consent_embedded_runtime` was deleted in FPE PR #12\n\n",
            "The later duplicate helper shadowed the canonical implementation.\n"
        );
        assert!(free_text_head_answered_by_response(response, head));

        let prose_only = concat!(
            "### Re: another queue item\n\n",
            "I noticed _load_identity_age_consent_embedded_runtime, but did not address it.\n"
        );
        assert!(!free_text_head_answered_by_response(prose_only, head));

        let different_identifier =
            "### Re: `_load_identity_age_consent_external_runtime`\n\nCompleted.\n";
        assert!(!free_text_head_answered_by_response(
            different_identifier,
            head
        ));
    }

    #[test]
    fn distinctive_identifier_heading_respects_explicit_deferral() {
        let head = "Please explain why _load_identity_age_consent_embedded_runtime was removed.";
        let response = concat!(
            "### Re: `_load_identity_age_consent_embedded_runtime`\n\n",
            "This remains queued and will be handled next.\n"
        );
        assert!(!free_text_head_answered_by_response(response, head));
    }

    #[test]
    fn free_text_head_answered_by_response_rejects_short_unlabeled_heads() {
        let resp = "### Re: x\n\n> - fix it now\n";
        assert!(!free_text_head_answered_by_response(resp, "fix it now"));
    }

    /// `#qheadresiduescope`: only text written after the anchor is newer, for an
    /// appended and a prepended exchange; a rewrite of older history widens the
    /// result instead of hiding a newer response.
    #[test]
    fn exchange_text_newer_than_returns_only_text_after_the_anchor() {
        let old =
            "### Re: a\n\n> **Queue prompt:** the widget crashes on save every time\n\nFixed.\n";
        let newer = "### Re: b\n\nUnrelated answer.\n";
        let appended = format!("{old}\n{newer}");
        let scoped = exchange_text_newer_than(&appended, old);
        assert!(scoped.contains("Unrelated answer."), "{scoped}");
        assert!(!scoped.contains("widget crashes"), "{scoped}");
        assert!(!free_text_head_answered_by_response(
            &scoped,
            "the widget crashes on save every time"
        ));

        let prepended = format!("{newer}\n{old}");
        let scoped = exchange_text_newer_than(&prepended, old);
        assert!(scoped.contains("Unrelated answer."), "{scoped}");
        assert!(!scoped.contains("widget crashes"), "{scoped}");

        assert_eq!(exchange_text_newer_than(old, old), "");
        let compacted = format!("Summary of earlier turns.\n\n{newer}");
        let scoped = exchange_text_newer_than(&compacted, old);
        assert!(scoped.contains("Summary of earlier turns."), "{scoped}");
        assert!(scoped.contains("Unrelated answer."), "{scoped}");
    }

    /// `#struckparentkids`: a reopened block (struck parent and answered
    /// child as context, then the new row) is proven answered by its own
    /// `> **Queue prompt:**` echo, so the answered strike closes it again;
    /// the echo of the earlier, closed block is not evidence for it.
    #[test]
    fn a_reopened_struck_parent_is_answered_by_its_own_echo() {
        let head = "~~#subagent-operator-request-handoff-integration-id~~\n  - ~~Describe the choices in more detail. Why do we need a SBX/STG id?~~\n  - I don't know if you understand that we moved onto project templates.";
        let echo = format_consumed_prompt_echo(&[head.to_string()], None);
        let response = format!("### Re: templates mint the id\n\n{echo}\nYou're right.\n");
        assert!(
            free_text_head_answered_by_response(&response, head),
            "{response}"
        );
        let earlier = "### Re: why SBX/STG need an integration id\n\n> **Queue prompt:** #subagent-operator-request-handoff-integration-id\n>   - Describe the choices in more detail. Why do we need a SBX/STG id?\n\nAn id is how haiven-api recognises the build.\n";
        assert!(
            !free_text_head_answered_by_response(earlier, head),
            "{earlier}"
        );
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
    fn subagent_completion_cell_is_not_direct_answer_evidence() {
        let head = "Fix the queue state regression after worker completion";
        let worker_response =
            format!("> **Queue prompt:** {head}\n\nImplemented the fix and all tests pass.");
        let completion = agent_doc_document::subagent_completion::cell_with_response(
            "subagent:queue-state",
            head,
            "attempt-key",
            &worker_response,
        )
        .unwrap();

        assert!(
            !free_text_head_has_explicit_answer_evidence(&completion, head),
            "a worker echo nested in binary completion feedback is a handoff, not a coordinator answer"
        );
        assert!(
            !free_text_head_answered_by_response(&completion, head),
            "completion feedback must not make the live row completed residue"
        );
        assert!(
            free_text_head_answered_by_response(&worker_response, head),
            "the same echo remains answer evidence when it is a direct response"
        );
    }

    #[test]
    fn short_head_words_in_an_unrelated_status_table_do_not_defer_its_exact_echo() {
        let response = concat!(
            "### Re: #upgrade — gpt-6.1-sol\n\n",
            "> **Queue prompt:** #upgrade\n\n",
            "Upgraded agent-doc and checked the current issues.\n\n",
            "| Issue | Status |\n",
            "| --- | --- |\n",
            "| 164 | `upgrade` recycles controller |\n",
            "| 172 | `**Deferred:**` leading head consumed |\n",
        );

        assert!(!response_defers_free_text_head(response, "#upgrade"));
    }

    #[test]
    fn short_exact_echo_followed_by_a_deferral_marker_still_defers() {
        let response = concat!(
            "### Re: #upgrade — gpt-6.1-sol\n\n",
            "> **Queue prompt:** #upgrade\n\n",
            "**Deferred:** wait for the release build.\n",
        );

        assert!(response_defers_free_text_head(response, "#upgrade"));
    }

    #[test]
    fn short_free_text_head_can_be_proven_by_exact_baseline_node() {
        let baseline = concat!(
            "<!-- agent:queue -->\n",
            "- staging deploy\n",
            "<!-- /agent:queue -->\n",
        );
        assert!(
            free_text_head_present_in_baseline(baseline, "staging deploy"),
            "baseline proof is an exact queue-node comparison, so it does not need the prose safety threshold"
        );
        assert!(!free_text_head_present_in_baseline(
            baseline,
            "production deploy"
        ));
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

    /// `#ftstrike-blockquote` — a head whose directive is followed by quoted
    /// prior-response context is answered by quoting the directive. Before this,
    /// the echo had to reproduce the embedded blockquote verbatim, so a correct
    /// answer left the head queued and the drain re-ran committed work.
    #[test]
    fn free_text_head_is_answered_by_quoting_the_directive_above_embedded_context() {
        let head = concat!(
            "We should use lazily instead of a TTL. Prefer the realtime document model over rebuilding from disk.\n",
            "> Descending instead of bailing means the caller *continues* — so every later read site re-paid the 3x6s self-heal.",
        );
        let response = concat!(
            "### Re: replica self-heal\n\n",
            "> **Queue prompt:** We should use lazily instead of a TTL. Prefer the realtime document model over rebuilding from disk.\n\n",
            "Done.\n",
        );
        assert!(
            free_text_head_answered_by_response(response, head),
            "quoting the operator's directive must strike the head without \
             reproducing the embedded blockquote context verbatim"
        );
    }

    /// The embedded-blockquote rule must not make heads strike incidentally: a
    /// response that quotes an unrelated directive still leaves the head queued.
    #[test]
    fn free_text_head_with_embedded_context_is_not_answered_by_an_unrelated_echo() {
        let head = concat!(
            "We should use lazily instead of a TTL. Prefer the realtime document model over rebuilding from disk.\n",
            "> Descending instead of bailing means the caller *continues*.",
        );
        let response = concat!(
            "### Re: something else\n\n",
            "> **Queue prompt:** Please rename the guard predicate to a decision-shaped name.\n\n",
            "Done.\n",
        );
        assert!(
            !free_text_head_answered_by_response(response, head),
            "an unrelated queue-prompt echo must not strike this head"
        );
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

    const NEST_PARENT: &str = "Sync the remote coder workspace before the smoke test";
    const NEST_CHILD: &str = "Where does this file live? My Coder agent is asking.";

    fn nest_document(queue: &str, exchange: &str) -> String {
        format!(
            "<!-- agent:exchange patch=append -->\n{exchange}<!-- /agent:exchange -->\n\n<!-- agent:run -->\n{queue}<!-- /agent:run -->\n"
        )
    }

    fn nest_queue() -> String {
        format!("- {NEST_PARENT}\n  - do [#tracked]\n  - {NEST_CHILD}\n")
    }

    fn vector_line() -> String {
        format!("{NEST_PARENT}: {NEST_CHILD}")
    }

    /// `#nestvector`: the vector answers the leaf. A long free-text ancestor
    /// whose prose the vector starts with stays unanswered.
    #[test]
    fn nest_vector_quote_answers_the_leaf_not_its_ancestor() {
        let doc = nest_document(&nest_queue(), "");
        let response = format!(
            "### Re: file\n\n> **Queue prompt:** {}\n\nDone.\n",
            vector_line()
        );
        assert!(queue_head_answered_by_response(&doc, &response, NEST_CHILD));
        assert!(!queue_head_answered_by_response(
            &doc,
            &response,
            NEST_PARENT
        ));
        assert!(free_text_head_answered_by_response(&response, NEST_PARENT));
        let bare = format!("### Re: file\n\n> **Queue prompt:** {NEST_CHILD}\n\nDone.\n");
        assert!(queue_head_answered_by_response(&doc, &bare, NEST_CHILD));
        assert!(!queue_head_answered_by_response(&doc, &bare, NEST_PARENT));
        let parent_only = format!("### Re: file\n\n> **Queue prompt:** {NEST_PARENT}\n\nDone.\n");
        assert!(queue_head_answered_by_response(
            &doc,
            &parent_only,
            NEST_PARENT
        ));
        assert!(!queue_head_answered_by_response(
            &doc,
            &parent_only,
            NEST_CHILD
        ));
    }

    #[test]
    fn deferred_nest_vector_quote_does_not_count_as_finished() {
        let doc = nest_document(&nest_queue(), "");
        let response = format!(
            "> **Queue prompt:** {}\n\n**Deferred:** the coder workspace is still down.\n",
            vector_line()
        );
        assert!(response_defers_queue_head(&doc, &response, NEST_CHILD));
        assert!(!response_defers_free_text_head(&response, NEST_CHILD));
        assert!(latest_queue_head_echo_is_deferral(
            &doc, &response, NEST_CHILD
        ));
        assert!(queue_head_answered_by_response_with_claim(
            &doc, &response, NEST_CHILD, true
        ));
    }

    #[test]
    fn embed_rewrites_a_bare_leaf_echo_into_the_nest_vector() {
        let queue = format!("- [#vscodecodersmoke]\n  - ~~{NEST_CHILD}~~\n");
        let exchange = format!(
            "### Re: coder file\n\n> **Queue prompt:** {NEST_CHILD}\n\nIt lives in the worktree.\n"
        );
        let doc = nest_document(&queue, &exchange);
        let updated = embed_consumed_prompt_in_response(
            &doc,
            &[NEST_CHILD.to_string()],
            Some("### Re: coder file"),
        );
        let vector = format!("#vscodecodersmoke: {NEST_CHILD}");
        assert!(updated.contains(&format!("> **Queue prompt:** {vector}")));
        assert!(!updated.contains(&format!("> **Queue prompt:** {NEST_CHILD}")));
        assert_eq!(updated.matches("> **Queue prompt:**").count(), 1);

        let already = nest_document(
            &queue,
            &format!(
                "### Re: coder file\n\n> **Queue prompt:** {vector}\n\nIt lives in the worktree.\n"
            ),
        );
        let updated = embed_consumed_prompt_in_response(
            &already,
            &[NEST_CHILD.to_string()],
            Some("### Re: coder file"),
        );
        assert_eq!(updated, already);

        let missing = nest_document(&queue, "### Re: coder file\n\nIt lives in the worktree.\n");
        let updated = embed_consumed_prompt_in_response(
            &missing,
            &[NEST_CHILD.to_string()],
            Some("### Re: coder file"),
        );
        assert!(updated.contains(&format!("> {vector}")));
        assert_eq!(updated.matches("> **Queue prompt:**").count(), 1);
    }

    #[test]
    fn embed_leaves_a_fenced_copy_of_the_leaf_and_inserts_the_vector() {
        let queue = format!("- [#vscodecodersmoke]\n  - {NEST_CHILD}\n");
        let exchange =
            format!("### Re: coder file\n\n```\n> **Queue prompt:** {NEST_CHILD}\n```\n\nDone.\n");
        let doc = nest_document(&queue, &exchange);
        let updated = embed_consumed_prompt_in_response(
            &doc,
            &[NEST_CHILD.to_string()],
            Some("### Re: coder file"),
        );
        assert!(updated.contains(&format!("```\n> **Queue prompt:** {NEST_CHILD}\n```")));
        assert!(updated.contains(&format!("#vscodecodersmoke: {NEST_CHILD}")));
    }

    #[test]
    fn submitted_queue_prompt_uses_the_live_rows_parent() {
        let content = "\
---
queue_active: true
---

<!-- agent:queue go -->
- https://github.com/haiven-dev/haiven-api/pull/736
  - ~~#rebase~~
- https://github.com/haiven-dev/haiven-api/pull/758
  - #rebase
<!-- /agent:queue -->
";
        assert_eq!(
            submitted_queue_prompt(content, "#rebase"),
            "https://github.com/haiven-dev/haiven-api/pull/758: #rebase"
        );
        assert_eq!(submitted_queue_prompt(content, "/clear"), "/clear");
        assert_eq!(
            submitted_continuation_prompt(content, "#rebase", |_| false),
            "https://github.com/haiven-dev/haiven-api/pull/758: #rebase"
        );
    }
}
