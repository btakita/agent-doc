//! Prompt completion pressure (`#promptpressure`).
//!
//! A turn that does not finish the prompt is not done. `/goal` already requeues
//! a hand-back that misses its declared final state. An ordinary prompt uses a
//! narrower gate: a prose blocker (a lead such as "Blocked" with no execution
//! evidence and no review structure) is requeued, and a real blocker is only
//! the review structure below. That structure is hoisted to the front of
//! `agent:review`, which the default template places ahead of `agent:exchange`,
//! so the incomplete work and how to finish it stay in front of the operator.
//! Exchange prose is not that surface. While the review item is open, the
//! named prompt is not drainable, dispatchable, or claimable.

use std::collections::HashSet;

use agent_doc_element::element;
use agent_doc_element_backlog::backlog::{self, PendingItem, PendingListMarker, PendingState};

/// Opens a prompt-blocker review block.
pub const OPEN_MARKER: &str = "<!-- agent-doc:prompt-blocker -->";
/// Closes a prompt-blocker review block.
pub const CLOSE_MARKER: &str = "<!-- /agent-doc:prompt-blocker -->";

/// The incomplete work, the step that finishes it, and the queue prompt it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptBlocker {
    /// Review item id. Worker-supplied when present, otherwise derived.
    pub id: String,
    /// One line naming the incomplete work.
    pub incomplete: String,
    /// The exact step that finishes the work.
    pub complete_by: String,
    /// The queue prompt this blocker holds open.
    pub prompt: String,
}

/// Parse one prompt-blocker review structure from a worker hand-back.
///
/// The structure is the marker pair plus a gated item whose continuation
/// carries `**Complete by:**` and `**Prompt:**`. Anything else is not a blocker.
pub fn parse_prompt_blocker(text: &str) -> Option<PromptBlocker> {
    let start = text.find(OPEN_MARKER)?;
    let after = start + OPEN_MARKER.len();
    let end = text[after..].find(CLOSE_MARKER)? + after;
    parse_blocker_body(&text[after..end])
}

/// True when `text` carries a complete prompt-blocker review structure.
pub fn response_contains_prompt_blocker(text: &str) -> bool {
    parse_prompt_blocker(text).is_some()
}

/// The one exchange line that points at the review item.
///
/// The essay stays out of `agent:exchange`. This sentence names the review id,
/// the incomplete work, and how to finish it.
pub fn exchange_pointer(blocker: &PromptBlocker) -> String {
    format!(
        "Blocked. The incomplete work is review #{}: {}. Complete by: {}.",
        blocker.id,
        one_line(&blocker.incomplete),
        one_line(&blocker.complete_by)
    )
}

/// True when `text` is a prose blocker that must not count as completing the prompt.
///
/// A review structure, the exchange pointer, and any response that already
/// carries execution evidence are not prose blockers. The lead is narrow on
/// purpose: words such as "error" or "missing" elsewhere in a finished
/// hand-back do not qualify.
pub fn prose_blocker_needs_pressure(text: &str) -> bool {
    if response_contains_prompt_blocker(text) || has_execution_evidence(text) {
        return false;
    }
    match first_substantive_line(text) {
        Some(line) if is_exchange_pointer_line(line) => false,
        Some(line) if is_blocker_lead(line) => true,
        _ => has_blocked_section(text),
    }
}

/// Normalized prompt identity used to tie a review item to a queue head.
pub fn normalize_prompt_text(text: &str) -> String {
    let stripped = agent_doc_element_queue::strip_queue_prompt_stable_id_annotation(text);
    let bare = agent_doc_document::queue_projection::strip_priority_markers(&stripped);
    bare.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

/// Open review item whose `**Prompt:**` line is `head`, if the head is blocked.
pub fn open_review_blocking_prompt(content: &str, head: &str) -> Option<PromptBlocker> {
    let want = normalize_prompt_text(head);
    if want.is_empty() {
        return None;
    }
    open_prompt_blockers(content)
        .into_iter()
        .find(|blocker| normalize_prompt_text(&blocker.prompt) == want)
}

/// True when an open review item holds `head` until the operator resolves it.
pub fn review_blocks_prompt(content: &str, head: &str) -> bool {
    open_review_blocking_prompt(content, head).is_some()
}

/// Normalized prompts held by open prompt-blocker review items.
pub fn review_blocked_prompts(content: &str) -> HashSet<String> {
    open_prompt_blockers(content)
        .into_iter()
        .map(|blocker| normalize_prompt_text(&blocker.prompt))
        .filter(|prompt| !prompt.is_empty())
        .collect()
}

/// Insert or replace `blocker` as the first `agent:review` item.
///
/// Creates the review component at its canonical slot when the document has
/// none. Repeating the same id replaces that item instead of duplicating it.
pub fn hoist_prompt_blocker(content: &str, blocker: &PromptBlocker) -> String {
    let content = ensure_review_component(content);
    let Ok(components) = element::parse(&content) else {
        return content;
    };
    let Some(component) = components
        .iter()
        .find(|component| element::is_review_component(&component.name))
    else {
        return content;
    };
    let body = upsert_blocker_body(component.content(&content), blocker);
    component.replace_content(&content, &body)
}

fn parse_blocker_body(body: &str) -> Option<PromptBlocker> {
    let mut incomplete = None;
    let mut id = None;
    let mut complete_by = None;
    let mut prompt = None;
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("<!--") {
            continue;
        }
        if let Some((parsed_id, text)) = parse_gated_line(trimmed) {
            id = parsed_id;
            incomplete = Some(one_line(&text));
            continue;
        }
        if let Some(value) = field_value(trimmed, "complete by") {
            complete_by = Some(one_line(&value));
        } else if let Some(value) = field_value(trimmed, "prompt") {
            prompt = Some(one_line(&value));
        }
    }
    let incomplete = incomplete.filter(|text| !text.is_empty())?;
    let complete_by = complete_by.filter(|text| !text.is_empty())?;
    let prompt = prompt.filter(|text| !text.is_empty())?;
    Some(PromptBlocker {
        id: accept_id(id, &prompt),
        incomplete,
        complete_by,
        prompt,
    })
}

fn parse_gated_line(line: &str) -> Option<(Option<String>, String)> {
    let rest = strip_list_marker(line)?;
    let rest = rest.strip_prefix("[/]")?.trim();
    if let Some(rest) = rest.strip_prefix("[#") {
        let (id, after) = rest.split_once(']')?;
        let text = after.trim();
        if id.is_empty() || text.is_empty() {
            return None;
        }
        return Some((Some(id.to_string()), text.to_string()));
    }
    if rest.is_empty() {
        return None;
    }
    Some((None, rest.to_string()))
}

fn strip_list_marker(line: &str) -> Option<&str> {
    let line = line.trim();
    if let Some(rest) = line.strip_prefix('-').or_else(|| line.strip_prefix('*')) {
        return Some(rest.trim());
    }
    let mut chars = line.chars();
    if chars.next().is_some_and(|ch| ch.is_ascii_digit()) {
        let rest = line.trim_start_matches(|ch: char| ch.is_ascii_digit());
        if let Some(rest) = rest.strip_prefix('.') {
            return Some(rest.trim());
        }
    }
    None
}

fn field_value(line: &str, name: &str) -> Option<String> {
    let line = strip_list_marker(line).unwrap_or(line).trim();
    let lower = line.to_ascii_lowercase();
    let needle = format!("**{name}:**");
    let index = lower.find(&needle)?;
    let value = line[index + needle.len()..].trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn accept_id(raw: Option<String>, prompt: &str) -> String {
    if let Some(raw) = raw {
        let id = backlog::normalize_pending_id(&raw);
        if backlog::is_valid_pending_id(&id) {
            return id;
        }
    }
    derived_id(prompt)
}

fn derived_id(prompt: &str) -> String {
    format!(
        "prompt-blocker-{}",
        short_hash(&normalize_prompt_text(prompt))
    )
}

fn short_hash(text: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")[..12].to_string()
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn first_substantive_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).find(|line| {
        !line.is_empty()
            && !line.starts_with("<!--")
            && !line.starts_with("### ")
            && !line.starts_with('>')
            && !line.starts_with("<details")
            && !line.starts_with("</details")
            && !line.starts_with("<summary")
            && !line.starts_with("</summary")
    })
}

fn is_exchange_pointer_line(line: &str) -> bool {
    line.trim()
        .to_ascii_lowercase()
        .starts_with("blocked. the incomplete work is review #")
}

fn is_blocker_lead(line: &str) -> bool {
    let mut lower = line.trim().to_ascii_lowercase();
    lower = lower
        .trim_start_matches(['*', '_', '`', '#', '-', ' '])
        .trim()
        .to_string();
    if let Some(rest) = lower.strip_prefix("**") {
        lower = rest.trim().to_string();
    }
    const LEADS: &[&str] = &[
        "blocked",
        "blocker:",
        "cannot complete",
        "can't complete",
        "unable to complete",
        "this prompt is not complete",
        "the prompt is not complete",
    ];
    LEADS.iter().any(|lead| lower.starts_with(lead))
}

fn has_blocked_section(text: &str) -> bool {
    text.lines().any(|line| {
        let trimmed = line.trim();
        // A bold label starts with `**`. Only a real list marker (`- ` / `* `)
        // is stripped, so `**Blocked:**` stays a section heading.
        let line = if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            strip_list_marker(trimmed).unwrap_or(trimmed)
        } else {
            trimmed
        };
        let lower = line.to_ascii_lowercase();
        lower.starts_with("**blocked:**") || lower.starts_with("**blocker:**")
    })
}

fn has_execution_evidence(text: &str) -> bool {
    if text.contains("```") || text.contains("~~~") {
        return true;
    }
    let lower = text.to_ascii_lowercase();
    if [
        "verification:",
        "what changed:",
        "commit / push:",
        "outcome:",
    ]
    .iter()
    .any(|label| lower.contains(label))
    {
        return true;
    }
    text.lines().any(|line| {
        let lower = line.to_ascii_lowercase();
        let command = line.contains('`')
            && (lower.contains("cargo ")
                || lower.contains("git ")
                || lower.contains("make ")
                || lower.contains("agent-doc "));
        command || has_commit_hash(line)
    })
}

fn has_commit_hash(line: &str) -> bool {
    let mut run = 0usize;
    for ch in line.chars() {
        if ch.is_ascii_hexdigit() {
            run += 1;
            if run >= 7 {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

fn open_prompt_blockers(content: &str) -> Vec<PromptBlocker> {
    let Ok(components) = element::parse(content) else {
        return Vec::new();
    };
    let mut blockers = Vec::new();
    for component in components
        .iter()
        .filter(|component| element::is_review_component(&component.name))
    {
        let (_, items, _) = backlog::parse_items(component.content(content));
        for item in items {
            if item.is_done() {
                continue;
            }
            let Some(prompt) = field_value_in(&item.continuation, "prompt") else {
                continue;
            };
            let Some(complete_by) = field_value_in(&item.continuation, "complete by") else {
                continue;
            };
            blockers.push(PromptBlocker {
                id: item.id,
                incomplete: one_line(&item.text),
                complete_by,
                prompt,
            });
        }
    }
    blockers
}

fn field_value_in(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|line| field_value(line, name))
}

fn ensure_review_component(content: &str) -> String {
    let Ok(components) = element::parse(content) else {
        return content.to_string();
    };
    if components
        .iter()
        .any(|component| element::is_review_component(&component.name))
    {
        return content.to_string();
    }
    let insert_at =
        element::canonical_managed_component_insertion_offset(content, &components, "review");
    let insert = "\n## Review\n\n<!-- agent:review -->\n<!-- /agent:review -->\n";
    let mut out = String::with_capacity(content.len() + insert.len());
    out.push_str(&content[..insert_at]);
    out.push_str(insert);
    out.push_str(&content[insert_at..]);
    out
}

fn upsert_blocker_body(body: &str, blocker: &PromptBlocker) -> String {
    let (body, _) = backlog::op_take_all_by_id(body, &blocker.id);
    let body = strip_empty_marker_pairs(&body);
    let block = render_block(blocker);
    let rest = body.trim_start_matches('\n');
    if rest.is_empty() {
        format!("\n{block}")
    } else {
        format!("\n{block}\n{rest}")
    }
}

fn render_block(blocker: &PromptBlocker) -> String {
    let item = PendingItem {
        marker: PendingListMarker::Bullet,
        id: blocker.id.clone(),
        state: PendingState::Gated,
        gate_type: None,
        text: blocker.incomplete.clone(),
        continuation: format!(
            "  - **Complete by:** {}\n  - **Prompt:** {}\n",
            blocker.complete_by, blocker.prompt
        ),
    };
    format!(
        "{OPEN_MARKER}\n{}\n{CLOSE_MARKER}\n",
        item.render().trim_end()
    )
}

fn strip_empty_marker_pairs(body: &str) -> String {
    let mut out = body.to_string();
    let mut search = 0usize;
    while search < out.len() {
        let Some(rel) = out[search..].find(OPEN_MARKER) else {
            break;
        };
        let start = search + rel;
        let after = start + OPEN_MARKER.len();
        let Some(close_rel) = out[after..].find(CLOSE_MARKER) else {
            break;
        };
        let close = after + close_rel;
        let interior = &out[after..close];
        if interior.contains("[/]") || interior.contains("[ ]") || interior.contains("[x]") {
            search = close + CLOSE_MARKER.len();
            continue;
        }
        let mut end = close + CLOSE_MARKER.len();
        if out[end..].starts_with('\n') {
            end += 1;
        }
        out.replace_range(start..end, "");
        search = start;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const STRUCTURE: &str = "\
<!-- agent-doc:prompt-blocker -->
- [/] [#prompt-blocker-widget] The widget export is unfinished
  - **Complete by:** land the export commit on fix/widget
  - **Prompt:** finish the widget
<!-- /agent-doc:prompt-blocker -->
";

    #[test]
    fn parses_a_review_structure_and_derives_a_missing_id() {
        let blocker = parse_prompt_blocker(STRUCTURE).expect("structure");
        assert_eq!(blocker.id, "prompt-blocker-widget");
        assert_eq!(blocker.incomplete, "The widget export is unfinished");
        assert_eq!(blocker.complete_by, "land the export commit on fix/widget");
        assert_eq!(blocker.prompt, "finish the widget");

        let derived = parse_prompt_blocker(
            "<!-- agent-doc:prompt-blocker -->\n- [/] The widget export is unfinished\n  - **Complete by:** land the export commit\n  - **Prompt:** finish the widget\n<!-- /agent-doc:prompt-blocker -->\n",
        )
        .expect("derived id");
        assert_eq!(derived.id, derived_id("finish the widget"));
        assert!(backlog::is_valid_pending_id(&derived.id));
        assert!(parse_prompt_blocker("Blocked. The widget export is unfinished.").is_none());
    }

    #[test]
    fn hoist_is_idempotent_and_stays_in_front_of_existing_review_items() {
        let blocker = parse_prompt_blocker(STRUCTURE).unwrap();
        let doc = "<!-- agent:queue -->\n- finish the widget\n<!-- /agent:queue -->\n\
                   <!-- agent:review -->\n- [/] [#other] Keep me\n<!-- /agent:review -->\n";
        let once = hoist_prompt_blocker(doc, &blocker);
        let twice = hoist_prompt_blocker(&once, &blocker);
        assert_eq!(once.matches(OPEN_MARKER).count(), 1, "{once}");
        assert_eq!(twice.matches(OPEN_MARKER).count(), 1, "{twice}");
        assert_eq!(
            twice.matches("[#prompt-blocker-widget]").count(),
            1,
            "{twice}"
        );
        assert!(
            twice.find("[#prompt-blocker-widget]").unwrap() < twice.find("[#other]").unwrap(),
            "{twice}"
        );
        assert!(review_blocks_prompt(&twice, "finish the widget"));
        assert!(review_blocks_prompt(&twice, "📥 finish the widget"));
        assert!(!review_blocks_prompt(&twice, "do the other thing"));
    }

    #[test]
    fn prose_pressure_is_a_narrow_lead_without_evidence() {
        assert!(prose_blocker_needs_pressure(
            "### Re: blocked\n\nBlocked by missing `OPENROUTER_API_KEY`; build cannot proceed."
        ));
        assert!(prose_blocker_needs_pressure(
            "**Summary**\nThe export is unfinished.\n\n**Blocked:** the API key is absent."
        ));
        assert!(!prose_blocker_needs_pressure(
            "### Re: done\n\nBlocked by a flaky test.\n\nVerification:\n- `cargo test -p agent-doc-queue`"
        ));
        assert!(!prose_blocker_needs_pressure(
            "error: the compiler failed on a missing import"
        ));
        assert!(!prose_blocker_needs_pressure(STRUCTURE));
        let pointer = exchange_pointer(&parse_prompt_blocker(STRUCTURE).unwrap());
        assert!(!prose_blocker_needs_pressure(&pointer), "{pointer}");
    }

    #[test]
    fn a_resolved_review_item_releases_the_prompt() {
        let content = "<!-- agent:review -->\n- [x] [#prompt-blocker-widget] done\n  - **Complete by:** land it\n  - **Prompt:** finish the widget\n<!-- /agent:review -->\n";
        assert!(!review_blocks_prompt(content, "finish the widget"));
    }
}
