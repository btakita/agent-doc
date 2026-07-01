//! Done/archive element descriptor.

use agent_doc_element::{
    ElementAuthority, ElementCompositionRole, ElementDescriptor, ElementRealtimeModel,
    ElementSchedulingRole, ElementShape, ElementSource, ElementWritePolicy,
};

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
}
