use anyhow::Result;

use agent_doc_frontmatter::{frontmatter, project_config};

const DEFAULT_TEMPLATE_STATUS_OPEN: &str = "\n\n## Status <!-- agent:status patch=replace -->\n";
const DEFAULT_TEMPLATE_STATUS_CLOSE: &str = "<!-- /agent:status -->\n";
const DEFAULT_TEMPLATE_COMPONENTS_AFTER_STATUS: &str = concat!(
    "\n## Exchange <!-- agent:exchange patch=append -->\n",
    "<!-- /agent:exchange -->\n",
    "\n## Run <!-- agent:run priority subagents clear -->\n",
    "<!-- /agent:run -->\n",
    "\n## Todo <!-- agent:todo priority -->\n",
    "<!-- /agent:todo -->\n",
    "\n## Backlog <!-- agent:backlog priority queue -->\n",
    "<!-- /agent:backlog -->\n",
    "\n## Review <!-- agent:review -->\n",
    "<!-- /agent:review -->\n",
    "\n## Icebox <!-- agent:icebox -->\n",
    "<!-- /agent:icebox -->\n",
    "\n## Done <!-- agent:done -->\n",
    "<!-- /agent:done -->\n",
    "\n## Notes <!-- agent:notes -->\n",
    "<!-- /agent:notes -->\n",
);

/// The default managed components. The status body starts with the
/// binary-owned queue marker legend (`#queuemarkerlegend`) so a new document
/// explains its `agent:run` prefixes before the first status write; later
/// status writes keep exactly one legend line.
fn default_template_components() -> String {
    let status_body = crate::status_projection::apply_queue_marker_legend("\n", true);
    format!(
        "{DEFAULT_TEMPLATE_STATUS_OPEN}{}{DEFAULT_TEMPLATE_STATUS_CLOSE}{DEFAULT_TEMPLATE_COMPONENTS_AFTER_STATUS}",
        status_body.trim_start_matches('\n')
    )
}

pub fn should_scaffold_empty_markdown(content: &str, extension: Option<&str>) -> bool {
    content.trim().is_empty() && extension == Some("md")
}

pub fn render_empty_template_scaffold(session_id: &str) -> String {
    format!(
        "---\nagent_doc_session: {session_id}\nagent_doc_format: template\nagent_doc_write: crdt\n---{}",
        default_template_components()
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
        "{}{}",
        content.trim_end(),
        default_template_components()
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
        let section_headers = scaffold
            .lines()
            .filter(|line| line.starts_with("## "))
            .collect::<Vec<_>>();
        assert_eq!(
            section_headers,
            [
                "## Status <!-- agent:status patch=replace -->",
                "## Exchange <!-- agent:exchange patch=append -->",
                "## Run <!-- agent:run priority subagents clear -->",
                "## Todo <!-- agent:todo priority -->",
                "## Backlog <!-- agent:backlog priority queue -->",
                "## Review <!-- agent:review -->",
                "## Icebox <!-- agent:icebox -->",
                "## Done <!-- agent:done -->",
                "## Notes <!-- agent:notes -->",
            ],
            "claim scaffolds keep every level-two heading on its component opening-tag line"
        );
        assert!(!scaffold.contains("## Queue"));
        assert!(
            scaffold.contains("<!-- agent:run priority subagents clear -->\n<!-- /agent:run -->"),
            "new claim scaffolds must use the canonical agent:run marker"
        );
        assert!(
            !scaffold.contains("<!-- agent:queue"),
            "new claim scaffolds must not emit the legacy agent:queue alias"
        );
        assert!(
            scaffold.contains(concat!(
                "<!-- /agent:run -->\n",
                "\n## Todo <!-- agent:todo priority -->\n<!-- /agent:todo -->\n",
                "\n## Backlog <!-- agent:backlog priority queue -->\n"
            )),
            "agent:todo must sit below agent:run and above agent:backlog"
        );

        let components = agent_doc_element::element::parse(&scaffold)
            .expect("default template components must form a valid component AST");
        let component_names = components
            .iter()
            .map(|component| component.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            component_names,
            [
                "status", "exchange", "queue", "todo", "backlog", "review", "icebox", "done",
                "notes",
            ],
            "claim scaffolds every canonical component in stable document order"
        );
        assert_eq!(components[0].patch_mode(), Some("replace"));
        assert_eq!(components[1].patch_mode(), Some("append"));
        let backlog = components
            .iter()
            .find(|component| component.name == "backlog")
            .expect("default scaffold must contain backlog");
        assert_eq!(backlog.attrs.get("priority").map(String::as_str), Some(""));
        assert_eq!(backlog.attrs.get("queue").map(String::as_str), Some(""));
        assert_eq!(
            components[2].attrs.get("priority").map(String::as_str),
            Some("")
        );
        assert_eq!(
            components[2].attrs.get("subagents").map(String::as_str),
            Some("")
        );
        assert_eq!(
            components[2].attrs.get("clear").map(String::as_str),
            Some("")
        );
        assert_eq!(components[2].attrs.len(), 3);
        let todo = components
            .iter()
            .find(|component| component.name == "todo")
            .expect("default scaffold includes todo");
        assert_eq!(todo.attrs.get("priority").map(String::as_str), Some(""));
        assert!(
            components[3..]
                .iter()
                .filter(|component| { component.name != "todo" && component.name != "backlog" })
                .all(|component| component.attrs.is_empty())
        );
        assert!(
            components[1..]
                .iter()
                .all(|component| component.content(&scaffold).is_empty()),
            "newly scaffolded components other than status must start empty"
        );
        // `#queuemarkerlegend`: status starts with exactly the legend line.
        assert_eq!(
            components[0].content(&scaffold),
            format!(
                "{}\n",
                agent_doc_frontmatter::frontmatter::queue_marker_legend_line()
            )
        );
    }

    #[test]
    fn scaffold_legend_survives_a_status_write_exactly_once() {
        let scaffold = render_empty_template_scaffold("session-1");
        let written =
            crate::status_projection::update_status_content(&scaffold, "Working on #a.").unwrap();
        let status = agent_doc_element::element::parse(&written)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "status")
            .unwrap();
        assert_eq!(
            status.content(&written),
            format!(
                "\nWorking on #a.\n\n{}\n",
                agent_doc_frontmatter::frontmatter::queue_marker_legend_line()
            )
        );
        let again =
            crate::status_projection::update_status_content(&written, "Working on #a.").unwrap();
        assert_eq!(again, written, "a repeated status write is a fixed point");
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
        assert!(
            scaffolded.contains(
                "## Run <!-- agent:run priority subagents clear -->\n<!-- /agent:run -->"
            )
        );
        assert!(!scaffolded.contains("## Queue"));
        assert!(!scaffolded.contains("<!-- agent:queue"));
        let component_names = agent_doc_element::element::parse(&scaffolded)
            .expect("default components must form a valid component AST")
            .into_iter()
            .map(|component| component.name)
            .collect::<Vec<_>>();
        assert_eq!(
            component_names,
            [
                "status", "exchange", "queue", "todo", "backlog", "review", "icebox", "done",
                "notes",
            ],
            "claim must use the same canonical component sequence for non-empty templates"
        );
        let queue = agent_doc_element::element::parse(&scaffolded)
            .expect("default components must remain parseable")
            .into_iter()
            .find(|component| component.name == "queue")
            .expect("default scaffold must contain queue");
        assert_eq!(queue.attrs.get("priority").map(String::as_str), Some(""));
        assert_eq!(queue.attrs.get("subagents").map(String::as_str), Some(""));
    }

    #[test]
    fn existing_legacy_queue_template_is_not_rewritten() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority -->\n- existing work\n<!-- /agent:queue -->\n",
        );

        assert!(
            scaffold_default_template_components(doc).unwrap().is_none(),
            "claim must preserve an established template instead of renaming legacy markers"
        );
        let components = agent_doc_element::element::parse(doc)
            .expect("legacy agent:queue remains a supported input spelling");
        let queue = components
            .iter()
            .find(|component| component.name == "queue")
            .expect("legacy queue alias keeps the stable internal queue identity");
        assert_eq!(queue.content(doc), "- existing work\n");
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
