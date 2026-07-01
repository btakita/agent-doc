//! Pure tracked-work projections used by orchestration policy.

use anyhow::{Context, Result};

use agent_doc_element::element::{self, is_backlog_component, is_tracked_work_component};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackedWorkFingerprint {
    pub component: Option<String>,
    pub baseline_hash: Option<String>,
    pub baseline_item_ids: Vec<String>,
}

impl TrackedWorkFingerprint {
    pub fn empty() -> Self {
        Self {
            component: None,
            baseline_hash: None,
            baseline_item_ids: Vec::new(),
        }
    }
}

pub fn tracked_work_fingerprint(content: &str) -> Result<TrackedWorkFingerprint> {
    let components = element::parse(content).context("failed to parse document components")?;
    let component = components
        .iter()
        .find(|component| is_backlog_component(&component.name))
        .or_else(|| {
            components
                .iter()
                .find(|component| is_tracked_work_component(&component.name))
        });
    let Some(component) = component else {
        return Ok(TrackedWorkFingerprint::empty());
    };

    let component_name = if is_backlog_component(&component.name) {
        "backlog".to_string()
    } else {
        component.name.clone()
    };
    let body = component.content(content);
    let baseline_hash = agent_doc_hash::content_hash(body);
    let (_, items, _) = agent_doc_element_backlog::backlog::parse_items(body);
    let baseline_item_ids = items
        .into_iter()
        .filter(|item| !item.is_done())
        .map(|item| item.id.trim().trim_start_matches('#').to_ascii_lowercase())
        .filter(|id| !id.is_empty())
        .collect();

    Ok(TrackedWorkFingerprint {
        component: Some(component_name),
        baseline_hash: Some(baseline_hash),
        baseline_item_ids,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprints_backlog_component_with_open_item_ids() {
        let doc = concat!(
            "# Session\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#Build-API] Build API\n",
            "- [/] [#Review-UI] Review UI\n",
            "- [x] [#done] Already done\n",
            "- [ ] no id\n",
            "<!-- /agent:backlog -->\n"
        );

        let fingerprint = tracked_work_fingerprint(doc).unwrap();

        assert_eq!(fingerprint.component.as_deref(), Some("backlog"));
        assert!(fingerprint.baseline_hash.is_some());
        assert_eq!(
            fingerprint.baseline_item_ids,
            vec!["build-api".to_string(), "review-ui".to_string()]
        );
    }

    #[test]
    fn falls_back_to_first_tracked_work_component() {
        let doc = concat!(
            "<!-- agent:review -->\n",
            "- [ ] [#Review-1] Check result\n",
            "<!-- /agent:review -->\n"
        );

        let fingerprint = tracked_work_fingerprint(doc).unwrap();

        assert_eq!(fingerprint.component.as_deref(), Some("review"));
        assert_eq!(fingerprint.baseline_item_ids, vec!["review-1".to_string()]);
    }

    #[test]
    fn empty_when_document_has_no_tracked_work_component() {
        let fingerprint = tracked_work_fingerprint("# Notes\n").unwrap();

        assert_eq!(fingerprint, TrackedWorkFingerprint::empty());
    }
}
