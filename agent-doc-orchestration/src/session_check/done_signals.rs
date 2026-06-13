use super::*;

pub(crate) fn open_tracked_work_ids(file: &Path) -> Result<Vec<String>> {
    let content = std::fs::read_to_string(file)?;
    let Ok(components) = crate::component::parse(&content) else {
        return Ok(Vec::new());
    };
    Ok(components
        .into_iter()
        .filter(|component| is_tracked_work_component(&component.name))
        .flat_map(|component| {
            let (_, items, _) = crate::pending::parse_items(component.content(&content));
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

    extract_pending_hash_ids(&normalized)
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

pub(crate) fn extract_pending_hash_ids(text: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let chars = text.char_indices().collect::<Vec<_>>();
    let mut idx = 0usize;

    while idx < chars.len() {
        let (byte_idx, ch) = chars[idx];
        if ch != '#' {
            idx += 1;
            continue;
        }

        let start = byte_idx + ch.len_utf8();
        let mut end = start;
        let mut cursor = idx + 1;
        while cursor < chars.len() {
            let (next_byte, next_ch) = chars[cursor];
            if next_ch.is_ascii_alphanumeric() || next_ch == '-' || next_ch == '_' {
                end = next_byte + next_ch.len_utf8();
                cursor += 1;
                continue;
            }
            break;
        }

        if end > start {
            let id = crate::pending::normalize_pending_id(&text[start..end]);
            if !ids.iter().any(|existing| existing == &id) {
                ids.push(id);
            }
        }
        idx = cursor.max(idx + 1);
    }

    ids
}

/// `#do-id-closeout-open-backlog`: extract the tracked-work ids named by an
/// explicit `do [#id]` / `do #id` prompt directive. Mirrors the binary-side
/// `tsift_graph::extract_do_targets` normalization (strip leading `❯`, an
/// optional bracketed annotation prefix like `[id]`, then require a `do `
/// prefix) so preflight can record the closeout expectation for those ids.
pub fn do_directive_target_ids(prompt_texts: &[String]) -> Vec<String> {
    let mut ids = Vec::new();
    for prompt in prompt_texts {
        for line in prompt.lines() {
            for id in do_directive_target_ids_in_line(line) {
                if !ids.iter().any(|existing| existing == &id) {
                    ids.push(id);
                }
            }
        }
    }
    ids
}

pub(crate) fn do_directive_target_ids_in_line(line: &str) -> Vec<String> {
    let mut normalized = line.trim().trim_start_matches('❯').trim();
    normalized = normalized
        .strip_prefix("- ")
        .or_else(|| normalized.strip_prefix("* "))
        .or_else(|| normalized.strip_prefix("+ "))
        .unwrap_or(normalized)
        .trim();
    // Queue priority pins (`:round_pushpin:` / `:pushpin:` / 📌) are cosmetic
    // annotations on the directive, not part of it: `:round_pushpin: [#id]`
    // targets `#id` exactly like the unpinned spelling
    // (#queue-user-edit-overwrite consumed-head accounting).
    let unpinned = crate::queue::strip_priority_markers(normalized);
    let mut normalized = unpinned.as_str();
    // Optional-`do` Stage 2: a `re [#id]` / `re #id` reference never targets a
    // tracked id — it is inert (no execute, no reap). Skip it before any id
    // extraction so the closeout guards do not expect a reference to be resolved.
    let lower_full = normalized.to_ascii_lowercase();
    if let Some(after_re) = lower_full.strip_prefix("re ") {
        let after_re = after_re.trim_start();
        if after_re.starts_with("[#") || after_re.starts_with('#') {
            return Vec::new();
        }
    }
    // Strip a leading non-id annotation prefix (`[label]`), but NOT a bare id
    // token `[#id]` — under the optional-`do` grammar that token IS the directive.
    if normalized.starts_with('[')
        && !normalized.starts_with("[#")
        && let Some(closing) = normalized.find(']')
    {
        normalized = normalized[closing + 1..].trim();
    }
    let lower = normalized.to_ascii_lowercase();
    // Explicit `do ` prefix keeps its original contract: extract every id target
    // named after the verb (e.g. `do [#a] then [#b]`).
    if let Some(rest) = lower.strip_prefix("do ") {
        return extract_pending_hash_ids(rest);
    }
    // Stage 2: the `do` verb is optional — a bare leading `[#id]` / `#id` token
    // is id-backed. A trailing `:` (`[#id]: note`) keeps the line inert prose.
    if leads_with_bare_id_token(&lower) {
        return extract_pending_hash_ids(&lower);
    }
    Vec::new()
}

/// Optional-`do` Stage 2: true when a normalized directive head leads with a
/// bare id token (`[#id]` or `#id`) that should execute / reap id-backed. A
/// trailing `:` after the token marks prose, not a directive (`[#id]: note`).
/// `lower` is expected lowercased and marker-stripped.
pub(crate) fn leads_with_bare_id_token(lower: &str) -> bool {
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

/// Open (`[ ]`/gated, not done) ids that live specifically in the live
/// `agent:backlog` component. The `expect_done_or_gate` guard keys off backlog
/// membership: `--done`, `--pending-gate`, reap, and icebox moves all remove an
/// id from `agent:backlog`, so an id still present here was never given a
/// lifecycle outcome.
pub(crate) fn open_backlog_ids(file: &Path) -> Result<Vec<String>> {
    let content = std::fs::read_to_string(file)?;
    let Ok(components) = crate::component::parse(&content) else {
        return Ok(Vec::new());
    };
    Ok(components
        .into_iter()
        .filter(|component| is_backlog_component(&component.name))
        .flat_map(|component| {
            let (_, items, _) = crate::pending::parse_items(component.content(&content));
            items
        })
        .filter(|item| !item.is_done())
        .map(|item| item.id)
        .filter(|id| !id.is_empty())
        .collect())
}

