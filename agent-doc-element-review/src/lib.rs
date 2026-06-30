//! Review element descriptor and pure review item projection.

use agent_doc_element::element;
use agent_doc_element::{
    ElementAuthority, ElementCompositionRole, ElementDescriptor, ElementRealtimeModel,
    ElementSchedulingRole, ElementShape, ElementSource, ElementWritePolicy,
};
use agent_doc_element_backlog::backlog;
use anyhow::{Context, Result};

pub const DESCRIPTOR: ElementDescriptor = ElementDescriptor {
    name: "review",
    aliases: &[],
    source: ElementSource::BuiltIn,
    shape: ElementShape::Component,
    authority: ElementAuthority::GranularTrackedWork,
    write_policy: ElementWritePolicy::GranularOnly,
    scheduling_role: ElementSchedulingRole::ReviewGate,
    realtime_model: ElementRealtimeModel::TrackedItems,
    composition_role: ElementCompositionRole::LocalOnly,
    realtime: true,
};

pub fn descriptor() -> ElementDescriptor {
    DESCRIPTOR
}

/// Find the `agent:review` component in already-read document content.
pub fn find_review_component_in_content(content: &str) -> Result<Option<element::Component>> {
    let components = element::parse(content).context("failed to parse components")?;
    Ok(components
        .into_iter()
        .find(|c| element::is_review_component(&c.name)))
}

fn insert_empty_review_after_backlog(content: &str) -> Result<String> {
    let components = element::parse(content).context("failed to parse components")?;
    if components
        .iter()
        .any(|c| element::is_review_component(&c.name))
    {
        return Ok(content.to_string());
    }
    let backlog = components
        .iter()
        .find(|c| element::is_backlog_component(&c.name))
        .context("document has no backlog/pending component for review insertion")?;
    let insert = "\n## Review\n\n<!-- agent:review -->\n<!-- /agent:review -->\n";
    let mut out = String::with_capacity(content.len() + insert.len());
    out.push_str(&content[..backlog.close_end]);
    out.push_str(insert);
    out.push_str(&content[backlog.close_end..]);
    Ok(out)
}

/// Ensure a document has an `agent:review` component after the backlog component.
///
/// Returns the possibly-updated content and the review component span in that
/// updated content. File IO stays with callers.
pub fn ensure_review_component_in_document(content: &str) -> Result<(String, element::Component)> {
    let content = insert_empty_review_after_backlog(content)?;
    let comp = find_review_component_in_content(&content)?
        .context("document has no review component after insertion")?;
    Ok((content, comp))
}

/// A token-efficient view of one gated `agent:review` item (`#review-list-query`).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ReviewItemView {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gate_type: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Extracted `NEXT:` annotation tail, if present (the actionable next step).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
    /// First line of the item text with tags stripped, bounded for a quick scan.
    pub summary: String,
}

/// Filter for [`review_item_views_from_content`]. `None` fields are unconstrained.
#[derive(Debug, Clone, Default)]
pub struct ReviewListFilter {
    pub gate_type: Option<String>,
    pub tag: Option<String>,
    /// `Some(true)` keeps only items with a `NEXT:` annotation, `Some(false)`
    /// only those without (the stale set to triage), `None` keeps all.
    pub has_next: Option<bool>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct UngateTasksReport {
    /// Gated review items scanned.
    pub scanned: usize,
    /// Review item ids a new backlog ungate task should be added for.
    pub added: Vec<String>,
    /// Review item ids already covered by an existing ungate task.
    pub skipped: Vec<String>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct UngateTasksPlan {
    pub report: UngateTasksReport,
    /// Backlog task bodies to append through the caller's document-write adapter.
    pub task_texts: Vec<String>,
}

/// Stable body text for a generated ungate backlog task, so re-runs dedup
/// against their own prior output idempotently.
fn ungate_task_text(normalized_id: &str) -> String {
    format!(
        "[recommended] Ungate review item #{} — validate and move to done",
        normalized_id
    )
}

/// Plan backlog follow-up tasks for gated `agent:review` items.
///
/// The returned task bodies are not written here. Callers own document IO and
/// decide how to apply the additions. The planning is idempotent: a review id
/// whose generated task or any `ungate` + `#id` backlog line already exists is
/// reported as skipped instead of added.
pub fn plan_ungate_tasks_for_review(content: &str) -> Result<UngateTasksPlan> {
    let components = element::parse(content)?;

    let gated_review_ids: Vec<String> = components
        .iter()
        .filter(|c| element::is_review_component(&c.name))
        .flat_map(|c| {
            let (_, items, _) = backlog::parse_items(c.content(content));
            items
        })
        .filter(|item| item.state == backlog::PendingState::Gated)
        .map(|item| backlog::normalize_pending_id(&item.id))
        .filter(|id| !id.is_empty())
        .collect();

    let backlog_text: String = components
        .iter()
        .filter(|c| element::is_backlog_component(&c.name))
        .map(|c| c.content(content).to_string())
        .collect::<Vec<_>>()
        .join("\n");

    let mut report = UngateTasksReport {
        scanned: gated_review_ids.len(),
        ..Default::default()
    };
    let mut seen = std::collections::HashSet::new();
    let mut task_texts: Vec<String> = Vec::new();
    for id in gated_review_ids {
        if !seen.insert(id.clone()) {
            continue;
        }
        let task_text = ungate_task_text(&id);
        let id_marker = format!("#{id}");
        let already_tracked = backlog_text.contains(&task_text)
            || backlog_text.lines().any(|line| {
                let lower = line.to_ascii_lowercase();
                lower.contains("ungate") && line.contains(&id_marker)
            });
        if already_tracked {
            report.skipped.push(id);
        } else {
            task_texts.push(task_text);
            report.added.push(id);
        }
    }

    Ok(UngateTasksPlan { report, task_texts })
}

/// Token-efficient projection of gated `agent:review` items.
///
/// Returns one [`ReviewItemView`] per gated item, with extracted hashtags and the
/// `NEXT:` annotation tail surfaced so a quick pass can triage a long review list
/// without reading the whole component. This is pure and owns no file IO.
pub fn review_item_views_from_content(
    content: &str,
    filter: &ReviewListFilter,
) -> Result<Vec<ReviewItemView>> {
    let components = element::parse(content)?;
    let mut views: Vec<ReviewItemView> = Vec::new();
    for c in components
        .iter()
        .filter(|c| element::is_review_component(&c.name))
    {
        let (_, items, _) = backlog::parse_items(c.content(content));
        for item in items
            .into_iter()
            .filter(|i| i.state == backlog::PendingState::Gated)
        {
            let id = backlog::normalize_pending_id(&item.id);
            if id.is_empty() {
                continue;
            }
            let tags = extract_review_tags(&item.text);
            let next = extract_review_next(&item.text);
            let first_line = item.text.lines().next().unwrap_or(&item.text).trim();
            let summary = bounded(first_line, 100);
            views.push(ReviewItemView {
                id,
                gate_type: item.gate_type.clone(),
                tags,
                next,
                summary,
            });
        }
    }
    Ok(apply_filter(views, filter))
}

fn apply_filter(mut views: Vec<ReviewItemView>, filter: &ReviewListFilter) -> Vec<ReviewItemView> {
    if let Some(gt) = filter.gate_type.as_deref() {
        views.retain(|v| v.gate_type.as_deref() == Some(gt));
    }
    if let Some(tag) = filter.tag.as_deref() {
        let want = if tag.starts_with('#') {
            tag.to_string()
        } else {
            format!("#{tag}")
        };
        views.retain(|v| v.tags.iter().any(|t| t == &want));
    }
    if let Some(has_next) = filter.has_next {
        views.retain(|v| v.next.is_some() == has_next);
    }
    views
}

/// Extract the hashtag tokens (`#foo-bar`) appearing in an item's text.
fn extract_review_tags(text: &str) -> Vec<String> {
    let mut tags = Vec::new();
    for raw in text.split_whitespace() {
        let tok = raw.trim_matches(|c: char| !(c.is_alphanumeric() || c == '#' || c == '-'));
        if tok.len() > 1
            && tok.starts_with('#')
            && tok[1..].chars().all(|c| c.is_alphanumeric() || c == '-')
            && !tags.iter().any(|t| t == tok)
        {
            tags.push(tok.to_string());
        }
    }
    tags
}

/// Extract the `NEXT:` annotation tail (case-insensitive), bounded.
fn extract_review_next(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let pos = lower.find("next:")?;
    let tail = text[pos + "next:".len()..].trim();
    let tail = tail.lines().next().unwrap_or(tail).trim();
    if tail.is_empty() {
        return None;
    }
    Some(bounded(tail, 160))
}

fn bounded(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max).collect();
    format!("{truncated}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_review_component_inserts_after_backlog() {
        let content = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#work] do work\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: prior\n",
            "<!-- /agent:exchange -->\n",
        );

        let (updated, review) = ensure_review_component_in_document(content).unwrap();
        assert!(element::is_review_component(&review.name));
        let backlog_end = updated.find("<!-- /agent:backlog -->").unwrap();
        let review_start = updated.find("<!-- agent:review -->").unwrap();
        let exchange_start = updated.find("<!-- agent:exchange -->").unwrap();
        assert!(
            backlog_end < review_start && review_start < exchange_start,
            "review component should be inserted after backlog and before exchange:\n{updated}"
        );
        assert!(
            updated[backlog_end..review_start].contains("## Review"),
            "review heading should be inserted before the review component:\n{updated}"
        );
        assert_eq!(review.content(&updated), "");
        assert!(updated.contains("<!-- agent:exchange -->"));
    }

    #[test]
    fn ensure_review_component_noops_when_review_exists() {
        let content = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#work] do work\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:review -->\n",
            "- [/] [#review] gated\n",
            "<!-- /agent:review -->\n",
        );

        let (updated, review) = ensure_review_component_in_document(content).unwrap();
        assert_eq!(updated, content);
        assert_eq!(review.content(&updated).trim(), "- [/] [#review] gated");
        assert!(
            find_review_component_in_content(&updated)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn ensure_review_component_errors_without_backlog() {
        let err = ensure_review_component_in_document("plain document").unwrap_err();
        assert!(
            err.to_string()
                .contains("document has no backlog/pending component for review insertion"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn review_item_views_extract_tags_next_and_filters() {
        let content = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:review -->\n",
            "- [/] [#aa11] #foo-tag first item summary. NEXT: do the thing\n",
            "- [/] [#bb22] second item, no next, #bar\n",
            "- [/release] [#cc33] release-gated item\n",
            "<!-- /agent:review -->\n",
        );

        let all = review_item_views_from_content(content, &ReviewListFilter::default()).unwrap();
        assert_eq!(all.len(), 3);

        let aa = all.iter().find(|v| v.id == "aa11").unwrap();
        assert!(aa.tags.contains(&"#foo-tag".to_string()), "{aa:?}");
        assert_eq!(aa.next.as_deref(), Some("do the thing"));

        let bb = all.iter().find(|v| v.id == "bb22").unwrap();
        assert!(bb.next.is_none());
        assert!(bb.tags.contains(&"#bar".to_string()), "{bb:?}");

        let f = ReviewListFilter {
            gate_type: Some("release".into()),
            ..Default::default()
        };
        let rel = review_item_views_from_content(content, &f).unwrap();
        assert_eq!(rel.len(), 1);
        assert_eq!(rel[0].id, "cc33");

        let f = ReviewListFilter {
            has_next: Some(true),
            ..Default::default()
        };
        let with_next = review_item_views_from_content(content, &f).unwrap();
        assert_eq!(with_next.len(), 1);
        assert_eq!(with_next[0].id, "aa11");

        let f = ReviewListFilter {
            has_next: Some(false),
            ..Default::default()
        };
        assert_eq!(
            review_item_views_from_content(content, &f).unwrap().len(),
            2
        );

        let f = ReviewListFilter {
            tag: Some("bar".into()),
            ..Default::default()
        };
        let tagged = review_item_views_from_content(content, &f).unwrap();
        assert_eq!(tagged.len(), 1);
        assert_eq!(tagged[0].id, "bb22");
    }

    #[test]
    fn plan_ungate_tasks_adds_each_unique_untracked_gated_review_item() {
        let content = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] unrelated open item\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:review -->\n",
            "- [/] [#rev1] gated review item one\n",
            "- [/] [#rev1] duplicate gated review item one\n",
            "- [/] [#rev2] gated review item two\n",
            "<!-- /agent:review -->\n",
        );

        let plan = plan_ungate_tasks_for_review(content).unwrap();
        assert_eq!(plan.report.scanned, 3);
        assert_eq!(plan.report.added, vec!["rev1", "rev2"]);
        assert!(plan.report.skipped.is_empty());
        assert_eq!(plan.task_texts.len(), 2);
        assert!(plan.task_texts[0].contains("Ungate review item #rev1"));
        assert!(plan.task_texts[1].contains("Ungate review item #rev2"));
    }

    #[test]
    fn plan_ungate_tasks_skips_existing_generated_or_operator_tasks() {
        let content = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#a] [recommended] Ungate review item #rev1 — validate and move to done\n",
            "- [ ] [#b] ungate #rev2 manually\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:review -->\n",
            "- [/] [#rev1] gated review item one\n",
            "- [/] [#rev2] gated review item two\n",
            "- [/] [#rev3] gated review item three\n",
            "<!-- /agent:review -->\n",
        );

        let plan = plan_ungate_tasks_for_review(content).unwrap();
        assert_eq!(plan.report.scanned, 3);
        assert_eq!(plan.report.skipped, vec!["rev1", "rev2"]);
        assert_eq!(plan.report.added, vec!["rev3"]);
        assert_eq!(plan.task_texts, vec![ungate_task_text("rev3")]);
    }
}
