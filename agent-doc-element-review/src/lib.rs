//! Review element descriptor and pure review item projection.

use agent_doc_element::element;
use agent_doc_element::{
    ElementAuthority, ElementCompositionRole, ElementDescriptor, ElementRealtimeModel,
    ElementSchedulingRole, ElementShape, ElementSource, ElementWritePolicy,
};
use agent_doc_element_backlog::backlog;
use anyhow::Result;

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
}
