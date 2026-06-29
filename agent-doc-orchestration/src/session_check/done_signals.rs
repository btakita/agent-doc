use super::*;

pub(crate) fn open_tracked_work_ids(file: &Path) -> Result<Vec<String>> {
    let content = std::fs::read_to_string(file)?;
    let Ok(components) = agent_doc_element::element::parse(&content) else {
        return Ok(Vec::new());
    };
    Ok(components
        .into_iter()
        .filter(|component| is_tracked_work_component(&component.name))
        .flat_map(|component| {
            let (_, items, _) =
                agent_doc_element_backlog::backlog::parse_items(component.content(&content));
            items
        })
        .filter(|item| !item.is_done())
        .map(|item| item.id)
        .collect())
}

pub(crate) fn response_clearly_completes_pending_id(response_text: &str, id: &str) -> bool {
    // Completion is signalled by a response HEADING whose topic RESOLVES to
    // exactly this id — never by a bare prose citation of `#id` in the body
    // (#pending-done-guard-false-positive). Mentioning a related/residual open id
    // in prose (e.g. "relates to #foo", "fixed alongside #bar") is a reference,
    // not a completion claim; the old prose-window heuristic read those as
    // completions and forced retry-with-suppression cycles. A heading match plus
    // a completion marker still distinguishes a real completion from a
    // halt/refusal response that merely names the head (#queue-strike-on-halt).
    if !response_heading_resolves_to_pending_id(response_text, id) {
        return false;
    }
    contains_completion_marker(&response_text.to_ascii_lowercase())
}

/// True when some `### Re:` response heading's topic resolves to `#id`. A
/// batch `do [#a] [#b] …` directive heading resolves to every bracketed id; a
/// titled `#id descriptive text` heading resolves only to its LEADING id (the
/// trailing words are prose). A heading that merely contains `#id` later in
/// descriptive prose — and any `#id` cited in the response BODY — never
/// resolves to it. This mirrors the exact-id queue-consume matching.
pub(crate) fn response_heading_resolves_to_pending_id(response_text: &str, id: &str) -> bool {
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
            // Batch do-directive: every bracketed `[#id]` is a completion target.
            let bracket_ids = extract_bracket_ids(do_list);
            if !bracket_ids.is_empty() {
                if bracket_ids.iter().any(|b| b == &id_lower) {
                    return true;
                }
                continue;
            }
            // No brackets — a single `do #id` form; leading id only.
            if leading_hash_id(do_list).as_deref() == Some(id_lower.as_str()) {
                return true;
            }
        } else if leading_hash_id(topic).as_deref() == Some(id_lower.as_str()) {
            return true;
        }
    }
    false
}

/// The leading `#id` token of `text` (optionally `[`-wrapped), or `None`.
pub(crate) fn leading_hash_id(text: &str) -> Option<String> {
    let t = text.strip_prefix('[').unwrap_or(text);
    let rest = t.strip_prefix('#')?;
    let id: String = rest
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        .collect();
    (!id.is_empty()).then_some(id)
}

/// All `[#id]` bracketed ids appearing in `text`, in order.
pub(crate) fn extract_bracket_ids(text: &str) -> Vec<String> {
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

pub(crate) fn contains_completion_marker(text: &str) -> bool {
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

pub fn inline_done_signal_ids(
    file: &Path,
    prompt_texts: &[String],
    auto_done: bool,
) -> Result<Vec<String>> {
    if prompt_texts.is_empty() {
        return Ok(Vec::new());
    }

    let open_ids = open_tracked_work_ids(file)?
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
    if open_ids.is_empty() {
        return Ok(Vec::new());
    }

    let single_review_id = if auto_done {
        single_open_review_item_id(file)?
    } else {
        None
    };
    let mut ids = Vec::new();

    for prompt in prompt_texts {
        for id in explicit_done_signal_ids(prompt) {
            if open_ids.contains(&id) && !ids.iter().any(|existing| existing == &id) {
                ids.push(id);
            }
        }

        if auto_done
            && plain_done_signal(prompt)
            && let Some(id) = single_review_id.as_deref()
            && open_ids.contains(id)
            && !ids.iter().any(|existing| existing == id)
        {
            ids.push(id.to_string());
        }
    }

    Ok(ids)
}

pub(crate) fn explicit_done_signal_ids(text: &str) -> Vec<String> {
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

pub(crate) fn plain_done_signal(text: &str) -> bool {
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

pub(crate) fn normalize_done_signal_text(text: &str) -> String {
    text.trim()
        .trim_start_matches('❯')
        .trim()
        .trim_start_matches("- ")
        .trim()
        .to_string()
}

/// Open (`[ ]`/gated, not done) ids that live specifically in the live
/// `agent:backlog` component. The `expect_done_or_gate` guard keys off backlog
/// membership: `--done`, `--pending-gate`, reap, and icebox moves all remove an
/// id from `agent:backlog`, so an id still present here was never given a
/// lifecycle outcome.
pub(crate) fn open_backlog_ids(file: &Path) -> Result<Vec<String>> {
    let content = std::fs::read_to_string(file)?;
    let Ok(components) = agent_doc_element::element::parse(&content) else {
        return Ok(Vec::new());
    };
    Ok(components
        .into_iter()
        .filter(|component| is_backlog_component(&component.name))
        .flat_map(|component| {
            let (_, items, _) =
                agent_doc_element_backlog::backlog::parse_items(component.content(&content));
            items
        })
        .filter(|item| !item.is_done())
        .map(|item| item.id)
        .filter(|id| !id.is_empty())
        .collect())
}
