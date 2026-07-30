//! Done/archive element descriptor.

use agent_doc_element::{
    ElementAuthority, ElementCompositionRole, ElementDescriptor, ElementRealtimeModel,
    ElementSchedulingRole, ElementShape, ElementSource, ElementWritePolicy,
    element::{self, Component},
};
use std::collections::HashSet;

pub const DESCRIPTOR: ElementDescriptor = ElementDescriptor {
    name: "done",
    aliases: &[],
    source: ElementSource::BuiltIn,
    shape: ElementShape::Component,
    authority: ElementAuthority::Archive,
    write_policy: ElementWritePolicy::ArchiveOnly,
    scheduling_role: ElementSchedulingRole::CompletionArchive,
    realtime_model: ElementRealtimeModel::Archive,
    composition_role: ElementCompositionRole::ArchiveTarget,
    realtime: true,
};

pub fn descriptor() -> ElementDescriptor {
    DESCRIPTOR
}

pub const DONE_SECTION_HEADING: &str = "## Completed / Reaped";
pub const EMPTY_DONE_COMPONENT: &str =
    "## Completed / Reaped\n\n<!-- agent:done -->\n<!-- /agent:done -->\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalDebrisPrune {
    pub content: String,
    pub removed_line_count: usize,
}

/// Remove invalid text below the terminal `agent:done` component when every
/// meaningful line is provably redundant with the managed document.
///
/// The done component is the document's terminal boundary. A CRDT merge can
/// nevertheless strand fragments below it. We only auto-prune when each
/// nonblank fragment is one of:
/// - an incomplete checklist stub;
/// - a same-id copy of a managed item above the boundary; or
/// - exact/subline text already present above the boundary.
///
/// Unique trailing text is left untouched so the structural guard can fail
/// closed instead of silently discarding it.
pub fn prune_proven_redundant_terminal_debris(document: &str) -> Option<TerminalDebrisPrune> {
    let done_end = terminal_done_prefix_end(document)?;

    let managed = &document[..done_end];
    let trailing = &document[done_end..];
    let meaningful = trailing
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if meaningful.is_empty()
        || !meaningful
            .iter()
            .all(|line| terminal_debris_line_is_redundant(managed, line))
    {
        return None;
    }

    Some(TerminalDebrisPrune {
        content: managed.to_string(),
        removed_line_count: trailing.lines().count(),
    })
}

fn terminal_done_prefix_end(document: &str) -> Option<usize> {
    const DONE_CLOSE: &str = "<!-- /agent:done -->";

    document
        .match_indices(DONE_CLOSE)
        .map(|(start, _)| {
            let marker_end = start + DONE_CLOSE.len();
            if document[marker_end..].starts_with("\r\n") {
                marker_end + 2
            } else if document[marker_end..].starts_with('\n') {
                marker_end + 1
            } else {
                marker_end
            }
        })
        .filter_map(|prefix_end| {
            let prefix = &document[..prefix_end];
            let components = element::parse(prefix).ok()?;
            let done = components
                .iter()
                .filter(|component| DESCRIPTOR.matches_name(&component.name))
                .max_by_key(|component| component.close_end)?;
            (done.close_end == prefix_end
                && !components
                    .iter()
                    .any(|component| component.open_start >= done.close_end))
            .then_some(prefix_end)
        })
        .last()
}

fn terminal_debris_line_is_redundant(managed: &str, line: &str) -> bool {
    let normalized_stub = line
        .trim_start_matches(['-', '*'])
        .trim()
        .trim_start_matches(['🚧', '⏭'])
        .trim();
    if matches!(normalized_stub, "[ ]" | "[/]" | "[x]" | "[X]") {
        return true;
    }
    if line.starts_with("~~") && line.contains("~~") && line.contains("auto-struck:") {
        return true;
    }
    if bare_boundary_identifier(line) {
        return true;
    }

    if let Some(id_start) = line.find("[#")
        && let Some(id_end) = line[id_start + 2..].find(']')
    {
        let id = &line[id_start + 2..id_start + 2 + id_end];
        if agent_doc_element_backlog::backlog::is_valid_pending_id(id)
            && managed.contains(&format!("[#{id}]"))
        {
            return true;
        }
    }

    let line = line.trim_matches('~').trim();
    if line.len() < 8 {
        return managed.contains(line);
    }
    managed.lines().map(str::trim).any(|candidate| {
        candidate == line
            || (line.len() >= 16 && candidate.contains(line))
            || (candidate.len() >= 16 && line.contains(candidate))
    })
}

fn bare_boundary_identifier(line: &str) -> bool {
    let line = line.trim();
    let bare_hex =
        (8..=64).contains(&line.len()) && line.bytes().all(|byte| byte.is_ascii_hexdigit());
    let uuid = line.len() == 36
        && line.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        });
    bare_hex || uuid
}

/// Insert the canonical done archive component after the last tracked-work
/// component in `content`.
pub fn insert_done_component_after_tracked_work(content: &str) -> Option<String> {
    let components = agent_doc_element::element::parse(content).ok()?;
    let anchor = components
        .iter()
        .filter(|c| agent_doc_element::element::is_tracked_work_component(&c.name))
        .max_by_key(|c| c.close_end)?;
    let insert_at = anchor.close_end;
    let mut result = String::with_capacity(content.len() + EMPTY_DONE_COMPONENT.len() + 2);
    result.push_str(&content[..insert_at]);
    if !result.ends_with("\n\n") {
        if !result.ends_with('\n') {
            result.push('\n');
        }
        result.push('\n');
    }
    result.push_str(EMPTY_DONE_COMPONENT);
    result.push_str(&content[insert_at..]);
    Some(result)
}

pub fn render_done_archive_entry(today: &str, id: &str, text: &str, continuation: &str) -> String {
    let mut entry = format!("- {today} [#{id}] {text}");
    entry.push('\n');
    if !continuation.is_empty() {
        entry.push_str(continuation);
        if !continuation.ends_with('\n') {
            entry.push('\n');
        }
    }
    entry
}

/// Extract each done item's own leading id from archive/done text.
///
/// Prose citations are ignored: a done item's identity is the first `[#id]` on
/// its list-item line, not any bracketed ids mentioned in continuation text.
pub fn collect_done_item_own_ids(text: &str) -> HashSet<String> {
    let mut ids = HashSet::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if !(trimmed.starts_with("- ") || trimmed.starts_with("* ")) {
            continue;
        }
        if let Some(start) = trimmed.find("[#") {
            let after = &trimmed[start + 2..];
            if let Some(end) = after.find(']') {
                let id = &after[..end];
                if agent_doc_element_backlog::backlog::is_valid_pending_id(id) {
                    ids.insert(id.to_ascii_lowercase());
                }
            }
        }
    }
    ids
}

/// Extract done-item ids from a parsed `agent:done` component.
pub fn collect_done_component_own_ids(document: &str, component: &Component) -> HashSet<String> {
    if !DESCRIPTOR.matches_name(&component.name) {
        return HashSet::new();
    }
    collect_done_item_own_ids(component.content(document))
}

/// Extract done-item ids from every `agent:done` component in a document.
pub fn collect_done_document_own_ids(document: &str) -> HashSet<String> {
    let Ok(components) = element::parse(document) else {
        return HashSet::new();
    };

    components
        .iter()
        .flat_map(|component| collect_done_component_own_ids(document, component))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_done_component_after_tracked_work_inserts_canonical_done_component() {
        let content = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#one] one\n",
            "<!-- /agent:backlog -->\n"
        );

        let updated = insert_done_component_after_tracked_work(content).unwrap();

        assert!(updated.contains(EMPTY_DONE_COMPONENT));
        assert!(updated.contains("<!-- /agent:backlog -->\n\n## Completed / Reaped"));
    }

    #[test]
    fn insert_done_component_after_tracked_work_uses_last_tracked_work_anchor() {
        let content = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#one] one\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:review -->\n",
            "- [ ] [#two] two\n",
            "<!-- /agent:review -->\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n"
        );

        let updated = insert_done_component_after_tracked_work(content).unwrap();

        let review_end = updated.find("<!-- /agent:review -->").unwrap();
        let done_start = updated.find("<!-- agent:done -->").unwrap();
        let exchange_start = updated.find("<!-- agent:exchange -->").unwrap();
        assert!(review_end < done_start);
        assert!(done_start < exchange_start);
    }

    #[test]
    fn insert_done_component_after_tracked_work_returns_none_without_anchor() {
        assert_eq!(
            insert_done_component_after_tracked_work("plain text\n"),
            None
        );
    }

    #[test]
    fn prunes_only_proven_redundant_debris_below_terminal_done() {
        let content = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#current] Current managed task with complete text\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:exchange -->\n",
            "The correlation id was cf2d65db-105b-472b-9d1e-de6638dc2eb6.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n",
            "~~Completed prompt~~ — auto-struck: answered this cycle (#ftstrike)\n",
            "- [ ] [#current] Superseded copy\n",
            "- [ ]\n",
            "410dfec2\n",
            "cf2d65db-105b-472b-9d1e-de6638dc2eb6\n",
        );

        let pruned = prune_proven_redundant_terminal_debris(content).unwrap();

        assert_eq!(
            pruned.content,
            content.split_once("~~Completed prompt~~").unwrap().0
        );
        assert_eq!(pruned.removed_line_count, 5);
    }

    #[test]
    fn preserves_unique_text_below_terminal_done() {
        let content = concat!(
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n",
            "Unique operator note that appears nowhere else.\n",
        );

        assert_eq!(prune_proven_redundant_terminal_debris(content), None);
    }

    #[test]
    fn prunes_redundant_structurally_invalid_debris_below_valid_done_prefix() {
        let content = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#queue-family] Add queue-family reactive flavors\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n",
            "queue-family reactive flavors\n",
            "- [ ] [#queue-family] Add queue-family reactive flavors\n",
            "<!-- /agent:backlog -->\n",
        );

        assert!(element::structural_corruption_reason(content).is_some());

        let pruned = prune_proven_redundant_terminal_debris(content).unwrap();

        assert_eq!(
            pruned.content,
            concat!(
                "<!-- agent:backlog -->\n",
                "- [ ] [#queue-family] Add queue-family reactive flavors\n",
                "<!-- /agent:backlog -->\n\n",
                "<!-- agent:done -->\n",
                "<!-- /agent:done -->\n",
            )
        );
        assert_eq!(pruned.removed_line_count, 3);
        assert!(element::structural_corruption_reason(&pruned.content).is_none());
    }

    #[test]
    fn does_not_treat_done_marker_inside_fence_as_terminal_prefix() {
        let content = concat!(
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n",
            "```\n",
            "<!-- /agent:done -->\n",
            "```\n",
        );

        assert_eq!(prune_proven_redundant_terminal_debris(content), None);
    }

    #[test]
    fn render_done_archive_entry_adds_single_trailing_newline() {
        assert_eq!(
            render_done_archive_entry("2026-07-01", "done1", "completed item", ""),
            "- 2026-07-01 [#done1] completed item\n"
        );
    }

    #[test]
    fn render_done_archive_entry_preserves_continuation() {
        assert_eq!(
            render_done_archive_entry("2026-07-01", "done1", "completed item", "  proof line\n"),
            "- 2026-07-01 [#done1] completed item\n  proof line\n"
        );
    }

    #[test]
    fn render_done_archive_entry_adds_missing_continuation_newline() {
        assert_eq!(
            render_done_archive_entry("2026-07-01", "done1", "completed item", "  proof line"),
            "- 2026-07-01 [#done1] completed item\n  proof line\n"
        );
    }

    #[test]
    fn collect_done_document_own_ids_extracts_from_done_component() {
        let content = concat!(
            "<!-- agent:done -->\n",
            "- [x] [#alpha] One thing\n",
            "- [x] [#bravo] Another\n",
            "<!-- /agent:done -->\n"
        );

        let ids = collect_done_document_own_ids(content);

        assert!(ids.contains("alpha"));
        assert!(ids.contains("bravo"));
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn collect_done_document_own_ids_ignores_non_done_components_and_prose_citations() {
        let content = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#open] Still open\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done -->\n",
            "- 2026-07-01 [#done1] Completed item mentioning [#open]\n",
            "  Continuation also cites [#not-own]\n",
            "<!-- /agent:done -->\n"
        );

        let ids = collect_done_document_own_ids(content);

        assert!(ids.contains("done1"));
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn collect_done_item_own_ids_extracts_archive_text_without_markers() {
        let archive = concat!(
            "- [x] [#archived1] First archived item\n",
            "- 2026-07-01 [#archived2] Second item mentioning [#citation]\n",
            "  Continuation cites [#continuation]\n",
        );

        let ids = collect_done_item_own_ids(archive);

        assert!(ids.contains("archived1"));
        assert!(ids.contains("archived2"));
        assert_eq!(ids.len(), 2);
    }
}
