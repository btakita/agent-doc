use anyhow::Context;
use std::collections::HashMap;

/// Build component-scoped replace patches for changed components present in
/// both document revisions.
pub fn component_replace_patches(
    before: &str,
    after: &str,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let before_components = agent_doc_element::element::parse(before)
        .context("failed to parse before document components for component replace patches")?;
    let after_components = agent_doc_element::element::parse(after)
        .context("failed to parse after document components for component replace patches")?;
    let before_by_name: HashMap<&str, &agent_doc_element::element::Component> = before_components
        .iter()
        .map(|component| (component.name.as_str(), component))
        .collect();
    let mut patches = Vec::new();
    for after_component in &after_components {
        let Some(before_component) = before_by_name.get(after_component.name.as_str()) else {
            continue;
        };
        let before_body = before_component.content(before);
        let after_body = after_component.content(after);
        if crate::transient_markers::normalize_transient_agent_doc_markers(before_body)
            == crate::transient_markers::normalize_transient_agent_doc_markers(after_body)
        {
            continue;
        }
        patches.push(serde_json::json!({
            "component": after_component.name,
            "content": after_body,
            "op": "replace",
        }));
    }
    Ok(patches)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drift_baseline() -> String {
        concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "> do #fix\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#fix]\n",
            "<!-- /agent:queue -->\n",
        )
        .to_string()
    }

    fn drift_recovered() -> String {
        concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "> do #fix\n",
            "### Re: do #fix\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#fix]\n",
            "<!-- /agent:queue -->\n",
        )
        .to_string()
    }

    fn compact_convergence_source() -> String {
        concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "> do #a\n",
            "### Re: do #a\n\n",
            "A long historical response body that compaction will archive.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#a]\n",
            "- do [#b]\n",
            "<!-- /agent:queue -->\n",
        )
        .to_string()
    }

    fn compact_convergence_compacted() -> String {
        concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "*Compacted. Content archived to `.agent-doc/archives/test.md`*\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#a]\n",
            "- do [#b]\n",
            "<!-- /agent:queue -->\n",
        )
        .to_string()
    }

    #[test]
    fn builds_replace_patch_for_changed_existing_component() {
        let patches = component_replace_patches(&drift_baseline(), &drift_recovered()).unwrap();

        assert_eq!(patches.len(), 1, "only exchange should need convergence");
        assert_eq!(patches[0]["component"], "exchange");
        assert_eq!(patches[0]["op"], "replace");
        assert!(
            patches[0]["content"]
                .as_str()
                .unwrap()
                .contains("### Re: do #fix"),
            "replace payload should carry the recovered response body: {patches:?}"
        );
    }

    #[test]
    fn skips_components_absent_from_before_document() {
        let before = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "same\n",
            "<!-- /agent:exchange -->\n",
        );
        let after = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "same\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- new\n",
            "<!-- /agent:backlog -->\n",
        );

        let patches = component_replace_patches(before, after).unwrap();

        assert!(patches.is_empty(), "new components are not replace patches");
    }

    #[test]
    fn normalizes_transient_markers_before_comparing() {
        let before = concat!(
            "<!-- agent:status patch=replace -->\n",
            "Working\n",
            "<!-- agent:boundary:one -->\n",
            "<!-- /agent:status -->\n",
        );
        let after = concat!(
            "<!-- agent:status patch=replace -->\n",
            "Working\n",
            "<!-- agent:boundary:two -->\n",
            "<!-- /agent:status -->\n",
        );

        let patches = component_replace_patches(before, after).unwrap();

        assert!(
            patches.is_empty(),
            "transient marker-only changes should not emit patches: {patches:?}"
        );
    }

    #[test]
    fn scopes_compaction_to_changed_component() {
        let patches = component_replace_patches(
            &compact_convergence_source(),
            &compact_convergence_compacted(),
        )
        .unwrap();

        assert_eq!(
            patches.len(),
            1,
            "only exchange changed during compaction; queue must not be patched: {patches:?}"
        );
        assert_eq!(patches[0]["component"], "exchange");
        assert_eq!(patches[0]["op"], "replace");
        assert!(
            patches[0]["content"]
                .as_str()
                .unwrap()
                .contains("*Compacted. Content archived"),
            "the exchange replace must carry the compacted summary body: {patches:?}"
        );
        assert!(
            !patches.iter().any(|patch| patch["component"] == "queue"),
            "a queue replace would clobber concurrent edits: {patches:?}"
        );
    }
}
