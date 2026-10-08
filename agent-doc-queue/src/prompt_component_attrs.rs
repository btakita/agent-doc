//! Component-scoped prompt policy shared by preflight and realtime steering.
//!
//! `agent:queue` and `agent:exchange` may both declare a default `preset` and
//! subagent dispatch intent on their opening marker. The policy is parsed from
//! structural components, so marker-like text in Markdown code ranges is never
//! treated as configuration.

use agent_doc_element::element;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromptComponentAttrs {
    pub preset: Option<String>,
    pub subagents: bool,
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
    let subagents = match name {
        "queue" => crate::subagent_intent::queue_subagents_mode(&component.attrs).is_some(),
        "exchange" => component.attrs.iter().any(|(key, value)| {
            crate::subagent_intent::is_queue_subagents_attr(key) && value.trim().is_empty()
        }),
        _ => false,
    };
    PromptComponentAttrs {
        preset: component
            .attrs
            .get("preset")
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
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
    fn valued_exchange_subagents_does_not_activate_boolean_dispatch() {
        let content = "<!-- agent:exchange subagents=2 -->\nfix it\n<!-- /agent:exchange -->\n";
        assert!(!prompt_component_attrs(content, "exchange").subagents);
    }
}
