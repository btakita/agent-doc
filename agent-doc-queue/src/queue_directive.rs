//! Queue directive target parsing.
//!
//! This module owns pure `agent:queue` prompt syntax for id-backed work targets:
//! explicit `do #id` / `do [#id]` directives, graph-task `do ...` target
//! extraction, and the optional-`do` bare leading `#id` forms. Callers provide
//! text; file-backed lifecycle checks stay outside this crate.

use std::collections::HashSet;

use agent_doc_document::queue_projection::strip_priority_markers;

/// Extract the first tracked-work id named by an explicit graph-task
/// `do [#id]` / `do #id` directive.
///
/// This is deliberately stricter than queue-head parsing below: bare `#id`
/// prompt heads are valid queue heads, but graph-task labels must use the
/// explicit `do ` verb before they feed tsift dispatch/context lookup.
pub fn explicit_do_directive_target_id(text: &str) -> Option<String> {
    explicit_do_directive_target_ids(text).into_iter().next()
}

/// Extract tracked-work ids named by an explicit graph-task `do ...` directive.
///
/// Normalization strips a leading prompt glyph (`❯`) and an optional bracketed
/// annotation prefix like `[prep]`, then requires a `do ` prefix. Every `#id` /
/// `[#id]` token after the verb is returned in first-seen order.
pub fn explicit_do_directive_target_ids(text: &str) -> Vec<String> {
    let mut normalized = text.trim().trim_start_matches('❯').trim();
    if normalized.starts_with('[')
        && let Some(closing) = normalized.find(']')
    {
        normalized = normalized[closing + 1..].trim();
    }
    let lower = normalized.to_ascii_lowercase();
    let Some(rest) = lower.strip_prefix("do ") else {
        return Vec::new();
    };
    agent_doc_element_backlog::backlog::extract_pending_hash_ids(rest)
}

/// `#do-id-closeout-open-backlog`: extract the tracked-work ids named by an
/// explicit `do [#id]` / `do #id` prompt directive.
///
/// Queue-head parsing accepts optional-`do` bare id forms in addition to the
/// explicit task directive grammar.
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
    let unpinned = strip_priority_markers(normalized);
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

/// True when `topic` resolves to exactly `#<head_id>` (optionally `do `-prefixed
/// or `[#id]` bracketed) with no trailing modifiers. Case-insensitive; `head_id`
/// is expected normalized lowercase by `queue_prompt_done_id`.
pub fn topic_resolves_to_exact_id(topic: &str, head_id: &str) -> bool {
    let norm = topic.trim().trim_start_matches('❯').trim();
    let norm = norm.strip_prefix("do ").unwrap_or(norm).trim();
    let inner = norm
        .strip_prefix("[#")
        .and_then(|rest| rest.strip_suffix(']'))
        .or_else(|| norm.strip_prefix('#'));
    matches!(inner, Some(id) if id.eq_ignore_ascii_case(head_id))
}

/// The `#id` directive tokens a head resolves to when the entire head is
/// composed of nothing but `do` plus one or more id directives (`[#id]` / `#id`),
/// whitespace-separated. Returns `None` as soon as any token is prose.
///
/// `#qmultiidstrike`: multi-id directive heads (`do [#a] [#b]`) are id-backed
/// regardless of id count and must not fall through to free-text positional
/// consumption before their referenced ids are reaped.
pub fn topic_resolves_to_only_id_directives(topic: &str) -> Option<Vec<String>> {
    let norm = topic.trim().trim_start_matches('❯').trim();
    let norm = norm.strip_prefix("do ").unwrap_or(norm).trim();
    if norm.is_empty() {
        return None;
    }
    let mut ids = Vec::new();
    for token in norm.split_whitespace() {
        let inner = token
            .strip_prefix("[#")
            .and_then(|rest| rest.strip_suffix(']'))
            .or_else(|| token.strip_prefix('#'))?;
        if inner.is_empty()
            || !inner
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        {
            return None;
        }
        ids.push(inner.to_ascii_lowercase());
    }
    Some(ids)
}

/// Narrow raw `do [#id]` directive target ids to those that must reach a
/// `--done`/`--pending-gate` lifecycle outcome this cycle.
///
/// Only ids still open in the live backlog are demanded, and ids auto-populated
/// by backlog-to-queue sync are excluded because they represent queue
/// maintenance work rather than user directives in the same cycle.
pub fn filter_expect_done_or_gate_ids(
    directive_ids: &[String],
    open_backlog_ids: &HashSet<String>,
    synced_queue_ids: &HashSet<String>,
) -> Vec<String> {
    let mut seen = HashSet::new();
    directive_ids
        .iter()
        .map(|id| agent_doc_element_backlog::backlog::normalize_pending_id(id))
        .filter(|id| open_backlog_ids.contains(id))
        .filter(|id| !synced_queue_ids.contains(id))
        .filter(|id| seen.insert(id.clone()))
        .collect()
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
    fn explicit_do_directive_target_ids_extract_common_graph_task_shapes() {
        assert_eq!(
            explicit_do_directive_target_id("do #agbr. spec"),
            Some("agbr".to_string())
        );
        assert_eq!(
            explicit_do_directive_target_id("do [#agbr]. spec"),
            Some("agbr".to_string())
        );
        assert_eq!(
            explicit_do_directive_target_id("[prep] do #agbr"),
            Some("agbr".to_string())
        );
        assert_eq!(explicit_do_directive_target_id("run tests"), None);
        assert_eq!(
            explicit_do_directive_target_ids("do [#x63e] [#v4v0]. spec-test"),
            vec!["x63e".to_string(), "v4v0".to_string()]
        );
        assert_eq!(
            explicit_do_directive_target_id("do #inline-done-signal. spec-test"),
            Some("inline-done-signal".to_string())
        );
    }

    #[test]
    fn explicit_do_directive_target_ids_reject_optional_do_queue_heads() {
        assert!(explicit_do_directive_target_ids("[#solo]").is_empty());
        assert!(explicit_do_directive_target_ids("#solo proceed").is_empty());
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

    #[test]
    fn topic_resolves_to_exact_id_rejects_modifiers() {
        assert!(topic_resolves_to_exact_id(
            "#spec-test-build-install-commit-push",
            "spec-test-build-install-commit-push"
        ));
        assert!(topic_resolves_to_exact_id("do [#foo]", "foo"));
        assert!(topic_resolves_to_exact_id("#Foo", "foo")); // case-insensitive
        // Trailing modifiers (#queue-strike-on-halt) must never resolve to the id.
        assert!(!topic_resolves_to_exact_id("#foo halt", "foo"));
        assert!(!topic_resolves_to_exact_id("#foo deferred", "foo"));
        assert!(!topic_resolves_to_exact_id("#other", "foo"));
    }

    #[test]
    fn multi_id_topic_resolves_to_only_id_directives() {
        assert_eq!(
            topic_resolves_to_only_id_directives("do #syncbarrier #crdtsvdom"),
            Some(vec!["syncbarrier".to_string(), "crdtsvdom".to_string()])
        );
        assert_eq!(
            topic_resolves_to_only_id_directives("do [#a] [#b]"),
            Some(vec!["a".to_string(), "b".to_string()])
        );
        assert_eq!(
            topic_resolves_to_only_id_directives("#foo"),
            Some(vec!["foo".to_string()])
        );
        assert_eq!(
            topic_resolves_to_only_id_directives("do #foo then ship it"),
            None
        );
        assert_eq!(topic_resolves_to_only_id_directives("re [#id]"), None);
        assert_eq!(topic_resolves_to_only_id_directives("just prose"), None);
        assert_eq!(topic_resolves_to_only_id_directives(""), None);
    }

    #[test]
    fn filter_expect_done_or_gate_ids_excludes_auto_synced_queue_ids() {
        // #queue-sync-auto-pending-done-guard-misfire: a cycle that works one
        // directive (#worked) while backlog-to-queue sync auto-populated sibling
        // ids into the active queue must demand only the genuine worked directive.
        let directive_ids = vec!["worked".to_string(), "a".to_string(), "b".to_string()];
        let open_backlog: HashSet<String> =
            ["worked", "a", "b"].iter().map(|s| s.to_string()).collect();
        let synced_queue_ids: HashSet<String> = ["a", "b"].iter().map(|s| s.to_string()).collect();

        let result =
            filter_expect_done_or_gate_ids(&directive_ids, &open_backlog, &synced_queue_ids);

        assert_eq!(result, vec!["worked".to_string()]);
    }

    #[test]
    fn filter_expect_done_or_gate_ids_keeps_open_directives_without_sync() {
        let directive_ids = vec![
            "Open".to_string(),
            "#open".to_string(),
            "resolved".to_string(),
        ];
        let open_backlog: HashSet<String> = ["open"].iter().map(|s| s.to_string()).collect();
        let synced_queue_ids = HashSet::new();

        let result =
            filter_expect_done_or_gate_ids(&directive_ids, &open_backlog, &synced_queue_ids);

        assert_eq!(result, vec!["open".to_string()]);
    }
}
