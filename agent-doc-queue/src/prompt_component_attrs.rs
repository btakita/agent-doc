//! Component-scoped prompt policy shared by preflight and realtime steering.
//!
//! `agent:queue` and `agent:exchange` may both declare a default `preset` and
//! subagent dispatch intent on their opening marker. The policy is parsed from
//! structural components, so marker-like text in Markdown code ranges is never
//! treated as configuration.

use agent_doc_element::element;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromptComponentAttrs {
    /// The trimmed `preset=` value exactly as parsed from the marker. Consumers
    /// must use [`Self::preset_names`] so a quoted value may carry more than one
    /// whitespace-separated preset reference.
    pub preset: Option<String>,
    pub subagents: bool,
}

impl PromptComponentAttrs {
    /// Preset references declared by `preset=`, in source order.
    ///
    /// The component parser removes the quotes around the whole attribute
    /// value. Split the remaining value on whitespace and also tolerate quote
    /// characters around individual references. Empty tokens are ignored;
    /// canonicalization, duplicate removal, and missing-preset handling belong
    /// to the resolver because bare aliases need the effective preset map.
    pub fn preset_names(&self) -> impl Iterator<Item = &str> {
        self.preset
            .as_deref()
            .into_iter()
            .flat_map(str::split_whitespace)
            .map(|name| name.trim_matches(|ch| ch == '\'' || ch == '"'))
            .filter(|name| !name.is_empty())
    }
}

/// The component's `preset` value, or `None` when there is no preset.
///
/// GH #227: `preset=""` is the operator's explicit "no preset" and is preserved
/// byte for byte in the document. Every resolver must read it as no preset, so
/// an empty (or whitespace-only) value is `None` exactly like an absent key.
pub fn component_preset(attrs: &std::collections::HashMap<String, String>) -> Option<&str> {
    attrs
        .get("preset")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
}

/// Read prompt defaults from the first structural component named `name`.
///
/// Queue subagent attributes retain their existing optional concurrency value.
/// Exchange subagent attributes are boolean because one exchange prompt is
/// dispatched at a time; invalid valued forms are rejected by attribute
/// validation and therefore do not activate dispatch here.
pub fn prompt_component_attrs(content: &str, name: &str) -> PromptComponentAttrs {
    let Ok(components) = element::parse(content) else {
        return PromptComponentAttrs::default();
    };
    let Some(component) = components.iter().find(|component| component.name == name) else {
        return PromptComponentAttrs::default();
    };
    prompt_component_attrs_for(component, name)
}

/// Read prompt defaults from a component already selected by the queue graph.
/// This avoids reintroducing first-occurrence policy in multi-queue callers.
pub fn prompt_component_attrs_for(
    component: &element::Component,
    name: &str,
) -> PromptComponentAttrs {
    let subagents = match name {
        "queue" => crate::subagent_intent::queue_subagents_mode(&component.attrs).is_some(),
        "exchange" => component.attrs.iter().any(|(key, value)| {
            crate::subagent_intent::is_queue_subagents_attr(key) && value.trim().is_empty()
        }),
        _ => false,
    };
    PromptComponentAttrs {
        preset: component_preset(&component.attrs).map(str::to_string),
        subagents,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exchange_prompt_attrs_are_structural_and_code_ranges_are_ignored() {
        let content = concat!(
            "```html\n<!-- agent:exchange subagents preset=\"#wrong\" -->\n```\n\n",
            "<!-- agent:exchange subagents preset=\"#review\" -->\n",
            "fix it\n",
            "<!-- /agent:exchange -->\n",
        );
        assert_eq!(
            prompt_component_attrs(content, "exchange"),
            PromptComponentAttrs {
                preset: Some("#review".to_string()),
                subagents: true,
            }
        );
    }

    #[test]
    fn quoted_preset_attribute_exposes_every_reference_in_source_order() {
        let content = concat!(
            "<!-- agent:queue preset=\"#push  '#auth' #push\" -->\n",
            "- release\n",
            "<!-- /agent:queue -->\n",
        );
        let attrs = prompt_component_attrs(content, "queue");
        assert_eq!(attrs.preset.as_deref(), Some("#push  '#auth' #push"));
        assert_eq!(
            attrs.preset_names().collect::<Vec<_>>(),
            vec!["#push", "#auth", "#push"]
        );

        let single_quoted = concat!(
            "<!-- agent:queue preset='#push #auth' -->\n",
            "- release\n",
            "<!-- /agent:queue -->\n",
        );
        assert_eq!(
            prompt_component_attrs(single_quoted, "queue")
                .preset_names()
                .collect::<Vec<_>>(),
            vec!["#push", "#auth"]
        );
    }

    /// GH #227: `preset=""` resolves to no preset on both prompt components.
    #[test]
    fn explicit_empty_preset_resolves_to_no_preset() {
        let queue =
            "<!-- agent:queue subagents preset=\"\" priority -->\n- a\n<!-- /agent:queue -->\n";
        assert_eq!(
            prompt_component_attrs(queue, "queue"),
            PromptComponentAttrs {
                preset: None,
                subagents: true,
            }
        );
        let exchange = "<!-- agent:exchange preset=\"\" -->\nfix it\n<!-- /agent:exchange -->\n";
        assert_eq!(prompt_component_attrs(exchange, "exchange").preset, None);

        let mut attrs = std::collections::HashMap::new();
        attrs.insert("preset".to_string(), "  ".to_string());
        assert_eq!(component_preset(&attrs), None);
        attrs.insert("preset".to_string(), "#ship".to_string());
        assert_eq!(component_preset(&attrs), Some("#ship"));
    }

    #[test]
    fn valued_exchange_subagents_does_not_activate_boolean_dispatch() {
        let content = "<!-- agent:exchange subagents=2 -->\nfix it\n<!-- /agent:exchange -->\n";
        assert!(!prompt_component_attrs(content, "exchange").subagents);
    }
}
