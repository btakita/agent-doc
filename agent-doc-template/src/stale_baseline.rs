//! Pure stale-baseline policy for template append components.
//!
//! Orchestration decides when to rebase and performs git/file effects. This
//! module only decides whether a candidate baseline is missing committed
//! append-component content from a snapshot/head document.

use std::collections::HashMap;

use crate::PatchBlock;

/// Components whose omitted `patch=` attribute defaults to append semantics.
pub fn is_append_mode_component(name: &str) -> bool {
    matches!(name, "exchange" | "findings")
}

pub fn patch_touches_exchange(patches: &[PatchBlock], unmatched: &str) -> bool {
    patches.iter().any(|patch| patch.name == "exchange") || !unmatched.trim().is_empty()
}

pub fn exchange_append_patch_can_rebase_to_head(
    patches: &[PatchBlock],
    unmatched: &str,
    mode_overrides: &HashMap<String, String>,
) -> bool {
    if mode_overrides
        .get("exchange")
        .is_some_and(|mode| mode == "replace")
    {
        return false;
    }
    patch_touches_exchange(patches, unmatched)
}

/// Detect whether a baseline is stale relative to the current snapshot.
///
/// Only append-mode components are checked: these grow monotonically and must
/// contain the snapshot's committed content. Replace-mode components are
/// user-editable and are skipped.
pub fn is_stale_baseline(baseline: &str, snapshot: &str) -> bool {
    let base_clean = strip_boundary_markers(baseline);
    let snap_clean = strip_boundary_markers(snapshot);

    if base_clean == snap_clean {
        return false;
    }

    if let (Ok(snap_components), Ok(base_components)) = (
        agent_doc_element::element::parse(snapshot),
        agent_doc_element::element::parse(baseline),
    ) && !snap_components.is_empty()
    {
        for snap_comp in &snap_components {
            let is_append = snap_comp
                .patch_mode()
                .map(|mode| mode == "append")
                .unwrap_or(is_append_mode_component(&snap_comp.name));
            if !is_append {
                continue;
            }
            let snap_content = strip_boundary_markers(snap_comp.content(snapshot).trim());
            if snap_content.is_empty() {
                continue;
            }
            if let Some(base_comp) = base_components
                .iter()
                .find(|component| component.name == snap_comp.name)
            {
                let base_content = strip_boundary_markers(base_comp.content(baseline).trim());
                if !base_content.contains(&snap_content) {
                    return true;
                }
            } else {
                return true;
            }
        }
        return false;
    }

    !base_clean.starts_with(&snap_clean)
}

fn strip_boundary_markers(content: &str) -> String {
    agent_doc_document::transient_markers::strip_boundary_markers(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_mode_component_defaults_include_exchange_and_findings() {
        assert!(is_append_mode_component("exchange"));
        assert!(is_append_mode_component("findings"));
    }

    #[test]
    fn replace_mode_components_are_not_default_append() {
        assert!(!is_append_mode_component("pending"));
        assert!(!is_append_mode_component("backlog"));
        assert!(!is_append_mode_component("status"));
        assert!(!is_append_mode_component("output"));
        assert!(!is_append_mode_component("todo"));
    }

    #[test]
    fn exchange_patch_rebase_requires_exchange_patch_or_unmatched_content() {
        let exchange = PatchBlock::new("exchange", "response");
        let status = PatchBlock::new("status", "ok");
        let mode_overrides = std::collections::HashMap::new();

        assert!(exchange_append_patch_can_rebase_to_head(
            &[exchange],
            "",
            &mode_overrides
        ));
        assert!(exchange_append_patch_can_rebase_to_head(
            &[],
            "plain response",
            &mode_overrides
        ));
        assert!(!exchange_append_patch_can_rebase_to_head(
            &[status],
            "   ",
            &mode_overrides
        ));
    }

    #[test]
    fn exchange_patch_rebase_respects_replace_override() {
        let exchange = PatchBlock::new("exchange", "response");
        let mut mode_overrides = std::collections::HashMap::new();
        mode_overrides.insert("exchange".to_string(), "replace".to_string());

        assert!(!exchange_append_patch_can_rebase_to_head(
            &[exchange],
            "plain response",
            &mode_overrides
        ));
    }

    #[test]
    fn identical_content_is_not_stale() {
        let doc = "<!-- agent:exchange patch=append -->\nResponse.\n<!-- /agent:exchange -->\n";
        assert!(!is_stale_baseline(doc, doc));
    }

    #[test]
    fn user_appended_text_is_not_stale() {
        let snapshot =
            "<!-- agent:exchange patch=append -->\nResponse.\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\nResponse.\nUser question\n<!-- /agent:exchange -->\n";
        assert!(!is_stale_baseline(baseline, snapshot));
    }

    #[test]
    fn user_edited_replace_component_is_not_stale() {
        let snapshot = "<!-- agent:status patch=replace -->\nOld status\n<!-- /agent:status -->\n\
                         <!-- agent:exchange patch=append -->\nResponse.\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:status patch=replace -->\nEdited status by user\n<!-- /agent:status -->\n\
                         <!-- agent:exchange patch=append -->\nResponse.\nNew question\n<!-- /agent:exchange -->\n";
        assert!(!is_stale_baseline(baseline, snapshot));
    }

    #[test]
    fn missing_committed_append_content_is_stale() {
        let snapshot = "<!-- agent:exchange patch=append -->\nCommitted response from agent.\n<!-- /agent:exchange -->\n";
        let baseline =
            "<!-- agent:exchange patch=append -->\nOld content only.\n<!-- /agent:exchange -->\n";
        assert!(is_stale_baseline(baseline, snapshot));
    }

    #[test]
    fn missing_append_component_is_stale() {
        let snapshot =
            "<!-- agent:exchange patch=append -->\nResponse.\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:other patch=append -->\nDifferent.\n<!-- /agent:other -->\n";
        assert!(is_stale_baseline(baseline, snapshot));
    }

    #[test]
    fn missing_replace_component_is_not_stale() {
        let snapshot = "<!-- agent:status patch=replace -->\nActive\n<!-- /agent:status -->\n\
                         <!-- agent:exchange patch=append -->\nResponse.\n<!-- /agent:exchange -->\n";
        let baseline =
            "<!-- agent:exchange patch=append -->\nResponse.\n<!-- /agent:exchange -->\n";
        assert!(!is_stale_baseline(baseline, snapshot));
    }

    #[test]
    fn boundary_markers_are_ignored() {
        let snapshot = "<!-- agent:exchange patch=append -->\nResponse.\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\nResponse.\n<!-- agent:boundary:xyz -->\nUser edit\n<!-- /agent:exchange -->\n";
        assert!(!is_stale_baseline(baseline, snapshot));
    }

    #[test]
    fn non_template_docs_fall_back_to_prefix_check() {
        let snapshot = "## Exchange\nResponse.\n";
        let baseline = "## Exchange\nResponse.\nNew question\n";
        assert!(!is_stale_baseline(baseline, snapshot));

        let stale = "## Exchange\nDifferent content.\n";
        assert!(is_stale_baseline(stale, snapshot));
    }

    #[test]
    fn empty_snapshot_append_component_is_skipped() {
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline =
            "<!-- agent:exchange patch=append -->\nUser added content\n<!-- /agent:exchange -->\n";
        assert!(!is_stale_baseline(baseline, snapshot));
    }

    #[test]
    fn default_exchange_is_append() {
        let snapshot = "<!-- agent:exchange -->\nResponse.\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange -->\nOld stuff.\n<!-- /agent:exchange -->\n";
        assert!(is_stale_baseline(baseline, snapshot));
    }

    #[test]
    fn default_findings_is_append() {
        let snapshot = "<!-- agent:findings -->\nCommitted finding.\n<!-- /agent:findings -->\n";
        let baseline = "<!-- agent:findings -->\nOld finding.\n<!-- /agent:findings -->\n";
        assert!(is_stale_baseline(baseline, snapshot));
    }
}
