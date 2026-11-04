//! Component-scoped prompt policy shared by preflight and realtime steering.
//!
//! `agent:queue` and `agent:exchange` may both declare a default `preset`
//! (`presets` is a compatibility alias) and subagent dispatch intent on their
//! opening marker. The policy is parsed from structural components, so
//! marker-like text in Markdown code ranges is never treated as configuration.

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
    /// See [`parse_preset_refs`] for the grammar.
    pub fn preset_refs(&self) -> Vec<PresetRef<'_>> {
        self.preset
            .as_deref()
            .map(parse_preset_refs)
            .unwrap_or_default()
    }

    /// Every preset name `preset=` declares, in source order: lookup names and
    /// inline-definition names alike. A resolver that looks these names up must
    /// first overlay [`Self::overlay_inline_presets`] so an inline definition
    /// resolves instead of reporting a missing preset.
    pub fn preset_names(&self) -> impl Iterator<Item = &str> {
        self.preset_refs().into_iter().map(PresetRef::name)
    }

    /// `effective` with every inline `#name: body` definition of this
    /// component's `preset=` defined on top (`#runpresetunresolved`). The
    /// inline body shadows a frontmatter entry of the same name.
    pub fn overlay_inline_presets(
        &self,
        effective: &agent_doc_frontmatter::frontmatter::PromptPresets,
    ) -> agent_doc_frontmatter::frontmatter::PromptPresets {
        let mut overlaid = effective.clone();
        for reference in self.preset_refs() {
            if let PresetRef::Inline { name, body } = reference {
                overlaid.define_inline(name, body);
            }
        }
        overlaid
    }
}

/// One reference in a prompt component's `preset=` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetRef<'a> {
    /// A preset name resolved through frontmatter `prompt_presets` (hashed or
    /// bare alias).
    Lookup(&'a str),
    /// `#name: body` (`#runpresetunresolved`): an inline preset definition.
    /// `body` is the operator's literal text after the colon, verbatim (it may
    /// itself contain `: `, `+`, and further words).
    Inline { name: &'a str, body: &'a str },
}

impl<'a> PresetRef<'a> {
    pub fn name(self) -> &'a str {
        match self {
            Self::Lookup(name) | Self::Inline { name, .. } => name,
        }
    }
}

/// Parse a `preset=` value into references, in source order.
///
/// The component parser removes the quotes around the whole attribute value.
/// The remaining value is a whitespace-separated list of preset references;
/// quote characters around an individual reference are tolerated and empty
/// tokens are ignored. The first token of the form `#name:` followed by more
/// text starts an inline definition: that text, through the end of the value,
/// is the body of the preset `#name`. Tokens before it remain lookups. A bare
/// trailing `#name:` with no body is a lookup of `#name`. Canonicalization,
/// duplicate removal, and missing-preset handling belong to the resolver
/// because bare aliases need the effective preset map.
pub fn parse_preset_refs(value: &str) -> Vec<PresetRef<'_>> {
    let mut refs = Vec::new();
    let mut rest = value;
    loop {
        let trimmed = rest.trim_start();
        if trimmed.is_empty() {
            break;
        }
        let end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
        let token = &trimmed[..end];
        rest = &trimmed[end..];
        let reference = token.trim_matches(|ch| ch == '\'' || ch == '"');
        if let Some(name) = inline_definition_name(reference) {
            let body = rest.trim();
            if body.is_empty() {
                refs.push(PresetRef::Lookup(name));
            } else {
                refs.push(PresetRef::Inline { name, body });
            }
            break;
        }
        if !reference.is_empty() {
            refs.push(PresetRef::Lookup(reference));
        }
    }
    refs
}

/// How to repair a `preset=` reference nothing defines. Shared by the preflight
/// warning and the fail-closed subagent brief so both name the same fix
/// (`#runpresetunresolved`).
pub const UNRESOLVED_PRESET_FIX: &str = "Define it under frontmatter `prompt_presets` \
(`'#name': <authorization text>`), or write the authorization inline on the marker as \
`preset=\"#name: <authorization text>\"`. A `preset=` value is a list of preset names, not \
prose: free text without a leading `#name:` is split into word tokens and each is looked up \
as a preset.";

/// Preset references on one prompt component that resolve to nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedComponentPresets {
    /// Component name as written after `agent:` (`run`, `queue`, `exchange`).
    pub component: String,
    /// The component's `id=`, when it declares one.
    pub id: Option<String>,
    /// Unresolved lookup names, in source order, deduplicated.
    pub names: Vec<String>,
}

/// Every prompt component (`agent:queue` / `agent:run` / `agent:loop` /
/// `agent:exchange`) whose `preset=` names a preset that neither `prompt_presets`
/// nor an inline `#name: body` definition on that component defines.
/// `is_defined` answers whether the effective preset map resolves a name
/// (hashed or bare alias).
pub fn unresolved_component_presets(
    content: &str,
    is_defined: impl Fn(&str) -> bool,
) -> Vec<UnresolvedComponentPresets> {
    let Ok(components) = element::parse(content) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for component in &components {
        // `agent:run` / `agent:loop` / `agent:queue` all parse as `queue`.
        if !matches!(component.name.as_str(), "queue" | "exchange") {
            continue;
        }
        let attrs = prompt_component_attrs_for(component, &component.name);
        let refs = attrs.preset_refs();
        let mut names: Vec<String> = Vec::new();
        for reference in &refs {
            let PresetRef::Lookup(name) = *reference else {
                continue;
            };
            let defined_inline = refs.iter().any(|other| {
                matches!(other, PresetRef::Inline { name: inline, .. }
                    if inline.trim_start_matches('#').eq_ignore_ascii_case(name.trim_start_matches('#')))
            });
            if defined_inline || is_defined(name) || names.iter().any(|seen| seen == name) {
                continue;
            }
            names.push(name.to_string());
        }
        if !names.is_empty() {
            out.push(UnresolvedComponentPresets {
                component: marker_name(content, component),
                id: component.attrs.get("id").cloned(),
                names,
            });
        }
    }
    out
}

/// The public marker spelling (`run`, `loop`, `queue`, `exchange`) as written,
/// rather than the internal canonical `queue`.
fn marker_name(content: &str, component: &element::Component) -> String {
    content
        .get(component.open_start..component.open_end)
        .and_then(|marker| marker.split_once("agent:"))
        .and_then(|(_, rest)| {
            rest.split(|ch: char| ch.is_whitespace() || ch == '-')
                .next()
        })
        .filter(|name| !name.is_empty())
        .unwrap_or(component.name.as_str())
        .to_string()
}

/// `#name` when `token` is `#name:`, a hashtag directly followed by one colon.
fn inline_definition_name(token: &str) -> Option<&str> {
    let name = token.strip_suffix(':')?;
    let bare = name.strip_prefix('#')?;
    (!bare.is_empty() && !bare.contains([':', '#'])).then_some(name)
}

/// The component's `preset` value, or `None` when there is no preset.
///
/// `presets=` is a compatibility alias for the easy-to-make plural spelling.
/// When both keys carry values, canonical `preset=` wins. The structural
/// parser omits empty attributes, so `preset="" presets="#alias"` resolves the
/// nonempty alias just as if the empty key were absent.
///
/// GH #227: `preset=""` is the operator's explicit "no preset" and is preserved
/// byte for byte in the document. Every resolver must read it as no preset, so
/// an empty (or whitespace-only) value is `None` exactly like an absent key.
pub fn component_preset(attrs: &std::collections::HashMap<String, String>) -> Option<&str> {
    attrs
        .get("preset")
        .or_else(|| attrs.get("presets"))
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

    /// `#runpresetunresolved`: the live `agent:run` marker wrote its
    /// authorization as free text after a preset name. It is one inline
    /// definition, not eleven word-token lookups.
    #[test]
    fn hashtag_colon_value_is_one_inline_preset_definition() {
        let content = concat!(
            "<!-- agent:run id=\"main\" subagents preset=\"#spec-test-build-install-commit-push: ",
            "approve all commit + integration + push: approve all worktree removals\" priority go -->\n",
            "- do it\n",
            "<!-- /agent:run -->\n",
        );
        let attrs = prompt_component_attrs(content, "queue");
        assert_eq!(
            attrs.preset_refs(),
            vec![PresetRef::Inline {
                name: "#spec-test-build-install-commit-push",
                body: "approve all commit + integration + push: approve all worktree removals",
            }]
        );
        assert_eq!(
            attrs.preset_names().collect::<Vec<_>>(),
            vec!["#spec-test-build-install-commit-push"]
        );
        let overlaid = attrs.overlay_inline_presets(&Default::default());
        assert_eq!(
            overlaid
                .get("#spec-test-build-install-commit-push")
                .map(String::as_str),
            Some("approve all commit + integration + push: approve all worktree removals")
        );

        // Lookups before the definition stay lookups.
        assert_eq!(
            parse_preset_refs("#push '#auth' #ship: and tag it"),
            vec![
                PresetRef::Lookup("#push"),
                PresetRef::Lookup("#auth"),
                PresetRef::Inline {
                    name: "#ship",
                    body: "and tag it"
                },
            ]
        );
        // A bare `#name` keeps meaning a lookup; a bodiless `#name:` is one too.
        assert_eq!(parse_preset_refs("#ship"), vec![PresetRef::Lookup("#ship")]);
        assert_eq!(
            parse_preset_refs("#ship:"),
            vec![PresetRef::Lookup("#ship")]
        );
        // Free text without a `#name:` head is still a list of lookups, so it
        // keeps failing closed at the resolver.
        assert_eq!(
            parse_preset_refs("commit + push"),
            vec![
                PresetRef::Lookup("commit"),
                PresetRef::Lookup("+"),
                PresetRef::Lookup("push"),
            ]
        );
        assert_eq!(
            parse_preset_refs("#a:b: text"),
            vec![PresetRef::Lookup("#a:b:"), PresetRef::Lookup("text")]
        );
    }

    /// The write-time lint names the marker as written and only the lookups
    /// nothing defines; an inline definition is never reported.
    #[test]
    fn unresolved_component_presets_reports_undefined_lookups_only() {
        let content = concat!(
            "<!-- agent:run id=\"main\" preset=\"#ship #nosuch #ship\" go -->\n- a\n<!-- /agent:run -->\n\n",
            "<!-- agent:loop preset=\"#ship: approve all commit\" -->\n- b\n<!-- /agent:loop -->\n\n",
            "<!-- agent:exchange preset=\"approve all\" -->\nhi\n<!-- /agent:exchange -->\n",
        );
        let unresolved = unresolved_component_presets(content, |name| name == "#ship");
        assert_eq!(
            unresolved,
            vec![
                UnresolvedComponentPresets {
                    component: "run".to_string(),
                    id: Some("main".to_string()),
                    names: vec!["#nosuch".to_string()],
                },
                UnresolvedComponentPresets {
                    component: "exchange".to_string(),
                    id: None,
                    names: vec!["approve".to_string(), "all".to_string()],
                },
            ]
        );
        assert!(unresolved_component_presets(content, |_| true).is_empty());
    }

    #[test]
    fn plural_presets_attribute_is_an_alias_with_singular_precedence() {
        let plural = concat!(
            "<!-- agent:queue presets=\"#push #auth\" -->\n",
            "- release\n",
            "<!-- /agent:queue -->\n",
        );
        assert_eq!(
            prompt_component_attrs(plural, "queue")
                .preset_names()
                .collect::<Vec<_>>(),
            vec!["#push", "#auth"]
        );

        let both = concat!(
            "<!-- agent:queue preset=\"#canonical\" presets=\"#alias\" -->\n",
            "- release\n",
            "<!-- /agent:queue -->\n",
        );
        assert_eq!(
            prompt_component_attrs(both, "queue").preset.as_deref(),
            Some("#canonical")
        );

        let empty_canonical = concat!(
            "<!-- agent:queue preset=\"\" presets=\"#alias\" -->\n",
            "- release\n",
            "<!-- /agent:queue -->\n",
        );
        assert_eq!(
            prompt_component_attrs(empty_canonical, "queue")
                .preset
                .as_deref(),
            Some("#alias")
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

    #[test]
    fn singular_subagent_marker_alias_preserves_prompt_intent() {
        for name in ["queue", "exchange"] {
            let content =
                format!("<!-- agent:{name} subagent -->\nfix it\n<!-- /agent:{name} -->\n");
            assert!(
                prompt_component_attrs(&content, name).subagents,
                "singular compatibility alias must not downgrade {name} ownership"
            );
        }
    }
}
