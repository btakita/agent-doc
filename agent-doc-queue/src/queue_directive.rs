//! Queue directive target parsing.
//!
//! This module owns pure `agent:queue` prompt syntax for id-backed work targets:
//! explicit `do #id` / `do [#id]` directives and the optional-`do` bare leading
//! `#id` forms. Callers provide text; file-backed lifecycle checks stay outside
//! this crate.

/// `#do-id-closeout-open-backlog`: extract the tracked-work ids named by an
/// explicit `do [#id]` / `do #id` prompt directive.
///
/// Mirrors the binary-side `tsift_graph::extract_do_targets` normalization
/// (strip leading `❯`, an optional bracketed annotation prefix like `[id]`, then
/// require a `do ` prefix) so preflight can record the closeout expectation for
/// those ids. Optional-`do` bare id forms are also accepted for queue heads.
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

pub fn do_directive_target_ids_in_line(line: &str) -> Vec<String> {
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
    let unpinned = crate::document_queue::strip_priority_markers(normalized);
    let mut normalized = unpinned.as_str();
    // Optional-`do` Stage 2: a `re [#id]` / `re #id` reference never targets a
    // tracked id — it is inert (no execute, no reap). Skip it before any id
    // extraction so closeout guards do not expect a reference to be resolved.
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
        return agent_doc_element_backlog::backlog::extract_pending_hash_ids(rest);
    }
    // Stage 2: the `do` verb is optional — a bare leading `[#id]` / `#id` token
    // is id-backed. A trailing `:` (`[#id]: note`) keeps the line inert prose.
    if leads_with_bare_id_token(&lower) {
        return agent_doc_element_backlog::backlog::extract_pending_hash_ids(&lower);
    }
    Vec::new()
}

/// Optional-`do` Stage 2: true when a normalized directive head leads with a
/// bare id token (`[#id]` or `#id`) that should execute / reap id-backed. A
/// trailing `:` after the token marks prose, not a directive (`[#id]: note`).
/// `lower` is expected lowercased and marker-stripped.
fn leads_with_bare_id_token(lower: &str) -> bool {
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
    fn do_directive_target_ids_extracts_bracketed_and_bare_forms() {
        let prompts = vec![
            "do [#alpha]".to_string(),
            "❯ do #beta".to_string(),
            "[queue] do #gamma".to_string(),
            "investigate #delta".to_string(),
        ];
        let ids = do_directive_target_ids(&prompts);
        assert_eq!(ids, vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn do_directive_target_ids_strips_queue_priority_pins() {
        // Queue maintenance pins lines with `:round_pushpin:` / `:pushpin:`;
        // the pinned spelling targets the same id as the unpinned one
        // (#queue-user-edit-overwrite consumed-head accounting).
        let prompts = vec![
            ":round_pushpin: [#pinned]".to_string(),
            "- :pushpin: do [#opin]".to_string(),
            "📌 #emoji proceed".to_string(),
        ];
        let ids = do_directive_target_ids(&prompts);
        assert_eq!(ids, vec!["pinned", "opin", "emoji"]);
    }

    #[test]
    fn do_directive_target_ids_optional_do_stage2_bare_and_reference_forms() {
        // Optional-`do` Stage 2: the `do` verb is optional for a bare leading id
        // token, and a `re` reference never targets an id.
        let prompts = vec![
            "[#solo]".to_string(),                      // bare bracketed -> id-backed
            "- [#listed] do the small fix".to_string(), // bare after list marker
            "#hashbare proceed".to_string(),            // bare hash token
            "re [#ref]".to_string(),                    // reference -> inert
            "re #ref2".to_string(),                     // reference -> inert
            "[#note]: just prose".to_string(),          // trailing `:` -> inert
            "see [#mention] for context".to_string(),   // not leading -> inert
            "do [#explicit]".to_string(),               // explicit still works
        ];
        let ids = do_directive_target_ids(&prompts);
        assert_eq!(ids, vec!["solo", "listed", "hashbare", "explicit"]);
    }
}
