use anyhow::Result;

use agent_doc_frontmatter::{frontmatter, project_config};

const DEFAULT_TEMPLATE_COMPONENTS: &str = "\n\n## Status\n\n<!-- agent:status patch=replace -->\n<!-- /agent:status -->\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n\n## Queue\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n\n## Backlog\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n\n## Icebox\n\n<!-- agent:icebox -->\n<!-- /agent:icebox -->\n";

pub fn should_scaffold_empty_markdown(content: &str, extension: Option<&str>) -> bool {
    content.trim().is_empty() && extension == Some("md")
}

pub fn render_empty_template_scaffold(session_id: &str) -> String {
    format!(
        "---\nagent_doc_session: {session_id}\nagent_doc_format: template\nagent_doc_write: crdt\n---{DEFAULT_TEMPLATE_COMPONENTS}"
    )
}

pub fn default_format_and_write_content(content: &str) -> Result<Option<String>> {
    let (fm, _) = frontmatter::parse(content)?;
    if fm.format.is_none() && fm.write_mode.is_none() && fm.mode.is_none() {
        let updated = frontmatter::set_format_and_write(
            content,
            frontmatter::AgentDocFormat::Template,
            frontmatter::AgentDocWrite::Crdt,
        )?;
        if updated != content {
            return Ok(Some(updated));
        }
    }
    Ok(None)
}

pub fn uses_template_format(content: &str) -> Result<bool> {
    let (fm, _) = frontmatter::parse(content)?;
    Ok(fm.resolve_mode().format == frontmatter::AgentDocFormat::Template)
}

pub fn scaffold_default_template_components(content: &str) -> Result<Option<String>> {
    if !uses_template_format(content)? {
        return Ok(None);
    }
    let has_status_or_exchange = agent_doc_element::element::parse(content)
        .map(|components| {
            components
                .iter()
                .any(|component| component.name == "status" || component.name == "exchange")
        })
        .unwrap_or(false);
    if has_status_or_exchange {
        return Ok(None);
    }
    Ok(Some(format!(
        "{}{DEFAULT_TEMPLATE_COMPONENTS}",
        content.trim_end()
    )))
}

pub fn merge_default_template_component_config(config: &mut project_config::ProjectConfig) -> bool {
    let mut changed = false;
    changed |= insert_default_component(config, "exchange", "append");
    changed |= insert_default_component(config, "findings", "append");
    changed |= insert_default_component(config, "status", "replace");
    changed
}

fn insert_default_component(
    config: &mut project_config::ProjectConfig,
    name: &str,
    patch: &str,
) -> bool {
    if config.components.contains_key(name) {
        return false;
    }
    config.components.insert(
        name.to_string(),
        project_config::ComponentConfig {
            patch: patch.to_string(),
            ..Default::default()
        },
    );
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_markdown_scaffold_uses_template_crdt_shell() {
        assert!(should_scaffold_empty_markdown(" \n", Some("md")));
        assert!(!should_scaffold_empty_markdown("x", Some("md")));
        assert!(!should_scaffold_empty_markdown("", Some("txt")));

        let scaffold = render_empty_template_scaffold("session-1");
        assert!(scaffold.contains("agent_doc_session: session-1"));
        assert!(scaffold.contains("agent_doc_format: template"));
        assert!(scaffold.contains("<!-- agent:status patch=replace -->"));
        assert!(scaffold.contains("<!-- agent:icebox -->"));
    }

    #[test]
    fn defaults_format_and_write_only_when_all_modes_absent() {
        let doc = "---\nagent_doc_session: s\n---\n\nBody\n";
        let updated = default_format_and_write_content(doc)
            .unwrap()
            .expect("missing mode should default");
        assert!(updated.contains("agent_doc_format: template"));
        assert!(updated.contains("agent_doc_write: crdt"));

        let explicit = "---\nagent_doc_format: template\n---\n\nBody\n";
        assert!(
            default_format_and_write_content(explicit)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn scaffolds_default_components_for_template_without_status_or_exchange() {
        let doc = "---\nagent_doc_format: template\n---\n\nIntro\n";
        let scaffolded = scaffold_default_template_components(doc)
            .unwrap()
            .expect("template without components should scaffold");
        assert!(scaffolded.starts_with("---\nagent_doc_format: template\n---\n\nIntro"));
        assert!(scaffolded.contains("## Status"));
        assert!(scaffolded.contains("<!-- agent:queue -->"));
    }

    #[test]
    fn preserves_template_with_existing_status_or_exchange() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Ready\n",
            "<!-- /agent:status -->\n"
        );
        assert!(scaffold_default_template_components(doc).unwrap().is_none());
    }

    #[test]
    fn merges_default_template_component_config_without_overwriting_existing_entries() {
        let mut config = project_config::ProjectConfig::default();
        config.components.insert(
            "status".to_string(),
            project_config::ComponentConfig {
                patch: "append".to_string(),
                ..Default::default()
            },
        );

        assert!(merge_default_template_component_config(&mut config));
        assert_eq!(config.components["status"].patch, "append");
        assert_eq!(config.components["exchange"].patch, "append");
        assert_eq!(config.components["findings"].patch, "append");
    }
}
