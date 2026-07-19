use agent_doc_document::queue_projection::{PRIORITIZED_MARKER, strip_priority_markers};
use anyhow::Result;

use crate::document_queue::{
    self, QueueEntry, QueuePrompt, has_auto_attr, has_stop_fence_at_head, marker_control, parse,
    render, resolve_activation, set_control_in_tag, strip_auto_from_tag, time_gate_at_head,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteDispatchQueueUpdate {
    pub content: String,
    pub prompt_text: String,
    pub appended: bool,
    pub already_present: bool,
    pub superseded: bool,
    pub component_created: bool,
    pub unparseable_queue_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteInactiveQueueHead {
    None,
    Dispatchable(String),
    Uncommitted(String),
}

pub fn route_prompt_text_for_change(change_text: &str) -> Option<String> {
    let mut lines = Vec::new();
    for line in change_text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("<!-- agent:boundary:") {
            continue;
        }
        let without_prompt_prefix = trimmed
            .strip_prefix('❯')
            .or_else(|| trimmed.strip_prefix('>'))
            .map(str::trim)
            .unwrap_or(trimmed);
        if !without_prompt_prefix.is_empty() {
            lines.push(without_prompt_prefix.to_string());
        }
    }
    let text = lines.join("\n").trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

pub fn active_auto_route_queue_prompt_texts(content: &str) -> Result<Vec<String>> {
    let (frontmatter, body) = agent_doc_frontmatter::frontmatter::parse(content)?;
    if frontmatter.queue_active != Some(true) {
        return Ok(Vec::new());
    }

    let components = agent_doc_element::element::parse(body)?;
    let Some(queue_component) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return Ok(Vec::new());
    };
    if !has_auto_attr(&queue_component.attrs) {
        return Ok(Vec::new());
    }

    let entries = parse(queue_component.content(body))?;
    Ok(document_queue::prompts(&entries)
        .into_iter()
        .filter_map(|prompt| route_prompt_text_for_change(&prompt.text))
        .collect())
}

pub fn strip_route_queue_state_for_boundary_compare(content: &str) -> String {
    let mut result = content
        .lines()
        .filter(|line| {
            let t = line.trim_start();
            // Both the canonical `queue:` control and the deprecated
            // `queue_active:` line are transient queue-maintenance state
            // (#queue-state-unify); normalize them away together.
            !t.starts_with("queue_active:") && !t.starts_with("queue:")
        })
        .collect::<Vec<_>>()
        .join("\n");
    if content.ends_with('\n') {
        result.push('\n');
    }
    if let Ok(components) = agent_doc_element::element::parse(&result) {
        for component in components.iter().rev() {
            if component.name == "queue" {
                result.replace_range(component.open_start..component.close_end, "");
            }
        }
    }
    agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(&result)
}

pub fn operator_prioritize_route_prompt(prompt_text: String) -> String {
    if crate::queue_command::is_slash_command(&prompt_text) {
        return prompt_text;
    }
    if document_queue::is_prioritized(&prompt_text) {
        prompt_text
    } else {
        format!(
            "{} {}",
            PRIORITIZED_MARKER,
            strip_priority_markers(&prompt_text)
        )
    }
}

pub fn dispatch_active_turn_queue_source(
    harness_binary: &str,
    blocker_reason: &str,
) -> Option<&'static str> {
    match (harness_binary, blocker_reason) {
        ("codex", "active codex turn") => Some("dispatch_only_codex_active_turn"),
        ("opencode", "opencode active turn") => Some("dispatch_only_opencode_active_turn"),
        ("claude", "active claude turn") => Some("dispatch_only_claude_active_turn"),
        ("claude", "claude artifact picker open") => Some("dispatch_only_claude_artifact_picker"),
        _ => None,
    }
}

pub fn prepare_route_dispatch_queue_update(
    original: &str,
    change_text: &str,
    priority: bool,
) -> Result<RouteDispatchQueueUpdate> {
    let prompt_text = route_prompt_text_for_change(change_text)
        .ok_or_else(|| anyhow::anyhow!("route queue prompt is empty"))?;
    let prompt_text = if priority {
        operator_prioritize_route_prompt(prompt_text)
    } else {
        prompt_text
    };
    let prompt_identity = strip_priority_markers(&prompt_text);
    let mut content = agent_doc_frontmatter::frontmatter::merge_queue_control(original, "go")?;
    let components = agent_doc_element::element::parse(&content)?;
    let mut component_created = false;
    let mut already_present = false;
    let mut appended = false;
    let mut superseded = false;
    let mut unparseable_queue_error = None;

    if let Some(queue_component) = components
        .iter()
        .find(|component| component.name == "queue")
        .cloned()
    {
        let body = &content[queue_component.open_end..queue_component.close_start];
        match parse(body) {
            Ok(mut entries) => {
                already_present = document_queue::prompts(&entries)
                    .iter()
                    .any(|prompt| strip_priority_markers(&prompt.text) == prompt_identity);
                if !already_present {
                    let active_prompt_count = entries
                        .iter()
                        .filter(|entry| matches!(entry, QueueEntry::Prompt(_)))
                        .count();
                    let replace_single_auto_prompt = !priority
                        && has_auto_attr(&queue_component.attrs)
                        && active_prompt_count == 1;
                    if replace_single_auto_prompt {
                        for entry in &mut entries {
                            if let QueueEntry::Prompt(prompt) = entry {
                                prompt.multiline = prompt_text.contains('\n');
                                prompt.text = prompt_text.clone();
                                superseded = true;
                                break;
                            }
                        }
                    } else {
                        let new_prompt = QueueEntry::Prompt(QueuePrompt {
                            multiline: prompt_text.contains('\n'),
                            indent: 0,
                            text: prompt_text.clone(),
                        });
                        if priority {
                            let insert_at = entries
                                .iter()
                                .position(|entry| matches!(entry, QueueEntry::Prompt(_)))
                                .unwrap_or(entries.len());
                            entries.insert(insert_at, new_prompt);
                        } else {
                            entries.push(new_prompt);
                        }
                        appended = true;
                    }
                }
                let rendered = render(&entries);
                content = queue_component.replace_content(&content, &rendered);
            }
            Err(parse_err) => {
                unparseable_queue_error = Some(parse_err.to_string());
                let new_rendered = render(std::slice::from_ref(&QueueEntry::Prompt(QueuePrompt {
                    multiline: prompt_text.contains('\n'),
                    indent: 0,
                    text: prompt_text.clone(),
                })));
                if body.lines().any(|line| line.trim() == new_rendered.trim())
                    || body.contains(prompt_text.as_str())
                {
                    already_present = true;
                    content = queue_component.replace_content(&content, body);
                } else {
                    let mut preserved = body.to_string();
                    if !preserved.is_empty() && !preserved.ends_with('\n') {
                        preserved.push('\n');
                    }
                    preserved.push_str(&new_rendered);
                    appended = true;
                    content = queue_component.replace_content(&content, &preserved);
                }
            }
        }
        content = activate_queue_component_marker(&content)?;
    } else {
        component_created = true;
        appended = true;
        content = insert_queue_component(&content, &prompt_text)?;
    }

    Ok(RouteDispatchQueueUpdate {
        content,
        prompt_text,
        appended,
        already_present,
        superseded,
        component_created,
        unparseable_queue_error,
    })
}

pub fn activate_existing_route_queue_content(original: &str) -> Result<String> {
    let content = agent_doc_frontmatter::frontmatter::merge_queue_control(original, "go")?;
    activate_queue_component_marker(&content)
}

pub fn inactive_route_queue_head(
    content: &str,
    queue_active: Option<bool>,
    committed_snapshot: Option<&str>,
) -> Result<RouteInactiveQueueHead> {
    if queue_active == Some(true) {
        return Ok(RouteInactiveQueueHead::None);
    }
    let components = agent_doc_element::element::parse(content)?;
    let Some(queue_component) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return Ok(RouteInactiveQueueHead::None);
    };
    let marker_control = marker_control(&queue_component.attrs);
    if matches!(
        marker_control,
        Some(agent_doc_frontmatter::frontmatter::QueueControl::Stop)
    ) {
        return Ok(RouteInactiveQueueHead::None);
    }
    let has_auto = has_auto_attr(&queue_component.attrs)
        || matches!(
            marker_control,
            Some(agent_doc_frontmatter::frontmatter::QueueControl::Start)
        );
    let body = &content[queue_component.open_end..queue_component.close_start];
    let entries = parse(body)?;
    let activation = resolve_activation(&entries, has_auto, false, false);
    if !activation.active
        || has_stop_fence_at_head(&activation.entries_after)
        || time_gate_at_head(&activation.entries_after).is_some()
    {
        return Ok(RouteInactiveQueueHead::None);
    }
    let Some(head) = document_queue::first_prompt(&activation.entries_after) else {
        return Ok(RouteInactiveQueueHead::None);
    };
    let head_text = head.text.clone();
    if !committed_snapshot_backs_queue_head(committed_snapshot, &head_text) {
        return Ok(RouteInactiveQueueHead::Uncommitted(head_text));
    }
    Ok(RouteInactiveQueueHead::Dispatchable(head_text))
}

pub fn committed_snapshot_backs_queue_head(
    committed_snapshot: Option<&str>,
    head_text: &str,
) -> bool {
    let Some(snapshot) = committed_snapshot else {
        return true;
    };
    let components = match agent_doc_element::element::parse(snapshot) {
        Ok(components) => components,
        Err(_) => return true,
    };
    let Some(queue_component) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return false;
    };
    let body = &snapshot[queue_component.open_end..queue_component.close_start];
    let entries = match parse(body) {
        Ok(entries) => entries,
        Err(_) => return true,
    };
    entries.iter().any(|entry| match entry {
        QueueEntry::Prompt(prompt) | QueueEntry::Completed(prompt) => prompt.text == head_text,
        _ => false,
    })
}

fn activate_route_queue_tag(tag: &str) -> String {
    set_control_in_tag(&strip_auto_from_tag(tag), Some("go"))
}

fn activate_queue_component_marker(content: &str) -> Result<String> {
    let components = agent_doc_element::element::parse(content)?;
    let Some(queue_component) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return Ok(content.to_string());
    };
    let open_tag = &content[queue_component.open_start..queue_component.open_end];
    let new_tag = activate_route_queue_tag(open_tag);
    let mut result = String::with_capacity(content.len());
    result.push_str(&content[..queue_component.open_start]);
    result.push_str(&new_tag);
    result.push_str(&content[queue_component.open_end..]);
    Ok(result)
}

fn insert_queue_component(content: &str, prompt_text: &str) -> Result<String> {
    let body = render(&[QueueEntry::Prompt(QueuePrompt {
        multiline: prompt_text.contains('\n'),
        indent: 0,
        text: prompt_text.to_string(),
    })]);
    let block = format!("<!-- agent:queue go -->\n{}<!-- /agent:queue -->\n\n", body);
    let components = agent_doc_element::element::parse(content)?;
    let insert_at = components
        .iter()
        .find(|component| agent_doc_element::element::is_tracked_work_component(&component.name))
        .map(|component| component.open_start)
        .or_else(|| {
            components
                .iter()
                .find(|component| component.name == "exchange")
                .map(|component| component.close_end)
        })
        .unwrap_or(content.len());
    let mut result = String::with_capacity(content.len() + block.len() + 2);
    result.push_str(&content[..insert_at]);
    if insert_at > 0 && !result.ends_with("\n\n") {
        if !result.ends_with('\n') {
            result.push('\n');
        }
        result.push('\n');
    }
    result.push_str(&block);
    result.push_str(&content[insert_at..]);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_prompt_text_strips_prompt_prefixes_and_boundaries() {
        assert_eq!(
            route_prompt_text_for_change("❯ do [#x]\n<!-- agent:boundary:head -->\n> next")
                .as_deref(),
            Some("do [#x]\nnext")
        );
        assert_eq!(
            route_prompt_text_for_change("  \n<!-- agent:boundary:head -->"),
            None
        );
    }

    #[test]
    fn active_auto_route_queue_prompt_texts_extracts_normalized_prompts() {
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- ❯ do [#alpha]\n",
            "<!-- /agent:queue -->\n"
        );

        assert_eq!(
            active_auto_route_queue_prompt_texts(content).unwrap(),
            vec!["do [#alpha]"]
        );
    }

    #[test]
    fn active_auto_route_queue_prompt_texts_requires_active_auto_queue() {
        let inactive = concat!(
            "---\nqueue_active: false\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#alpha]\n",
            "<!-- /agent:queue -->\n"
        );
        let manual = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue -->\n",
            "- do [#alpha]\n",
            "<!-- /agent:queue -->\n"
        );
        let missing = "---\nqueue_active: true\n---\n\nNo queue.\n";

        assert!(
            active_auto_route_queue_prompt_texts(inactive)
                .unwrap()
                .is_empty()
        );
        assert!(
            active_auto_route_queue_prompt_texts(manual)
                .unwrap()
                .is_empty()
        );
        assert!(
            active_auto_route_queue_prompt_texts(missing)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn strip_route_queue_state_for_boundary_compare_removes_transient_queue_state() {
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "queue_active: true\n",
            "queue: go\n",
            "---\n\n",
            "Stable text.\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#alpha]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:boundary:head -->\n",
            "More stable text.\n"
        );

        let stripped = strip_route_queue_state_for_boundary_compare(content);

        assert!(stripped.contains("Stable text."));
        assert!(stripped.contains("More stable text."));
        assert!(!stripped.contains("queue_active:"));
        assert!(!stripped.contains("queue:"));
        assert!(!stripped.contains("agent:queue"));
        assert!(!stripped.contains("agent:boundary"));
    }

    #[test]
    fn priority_prompt_is_operator_pinned_except_slash_command() {
        assert_eq!(
            operator_prioritize_route_prompt("do [#x]".to_string()),
            "📌 do [#x]"
        );
        assert_eq!(
            operator_prioritize_route_prompt(":round_pushpin: do [#x]".to_string()),
            "📌 do [#x]"
        );
        assert_eq!(
            operator_prioritize_route_prompt("/clear".to_string()),
            "/clear"
        );
    }

    #[test]
    fn active_turn_blockers_are_queueable_for_prompt_bearing_reroutes() {
        assert_eq!(
            dispatch_active_turn_queue_source("codex", "active codex turn"),
            Some("dispatch_only_codex_active_turn")
        );
        assert_eq!(
            dispatch_active_turn_queue_source("opencode", "opencode active turn"),
            Some("dispatch_only_opencode_active_turn")
        );
        assert_eq!(
            dispatch_active_turn_queue_source("claude", "active claude turn"),
            Some("dispatch_only_claude_active_turn")
        );
        assert_eq!(
            dispatch_active_turn_queue_source("claude", "claude artifact picker open"),
            Some("dispatch_only_claude_artifact_picker")
        );
        assert_eq!(
            dispatch_active_turn_queue_source("codex", "codex hook review prompt"),
            None,
            "hook review requires an explicit operator decision, not auto-queueing"
        );
        assert_eq!(
            dispatch_active_turn_queue_source("codex", "queued draft in composer"),
            None,
            "drafted prompt input must not be overwritten by route queueing"
        );
    }

    #[test]
    fn route_dispatch_update_creates_queue_before_tracked_work() {
        let original = concat!(
            "---\nagent_doc_format: template\nqueue_active: false\n---\n\n",
            "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n- [ ] [#x] Work.\n<!-- /agent:backlog -->\n"
        );
        let update = prepare_route_dispatch_queue_update(original, "❯ do [#x]", false).unwrap();
        assert!(update.component_created);
        assert!(update.appended);
        assert_eq!(update.prompt_text, "do [#x]");
        assert!(update.content.contains("queue: go"));
        assert!(
            update
                .content
                .contains("<!-- agent:queue go -->\n- do [#x]\n<!-- /agent:queue -->")
        );
        assert!(
            update.content.find("<!-- agent:queue go -->").unwrap()
                < update.content.find("<!-- agent:backlog -->").unwrap()
        );
    }

    #[test]
    fn route_dispatch_update_priority_preempts_existing_prompts() {
        let original = concat!(
            "---\nagent_doc_format: template\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "preset #spec\n",
            "- first\n",
            "- second\n",
            "<!-- /agent:queue -->\n"
        );
        let update = prepare_route_dispatch_queue_update(original, "manual preempt", true).unwrap();
        assert!(update.appended);
        assert!(!update.superseded);
        assert!(update.content.contains("<!-- agent:queue go -->"));
        assert!(!update.content.contains("agent:queue auto"));
        assert!(
            update
                .content
                .contains("preset #spec\n- 📌 manual preempt\n- first\n- second")
        );
    }

    #[test]
    fn route_dispatch_update_supersedes_single_nonpriority_auto_prompt() {
        let original = concat!(
            "---\nagent_doc_format: template\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- old\n",
            "<!-- /agent:queue -->\n"
        );
        let update = prepare_route_dispatch_queue_update(original, "new", false).unwrap();
        assert!(update.superseded);
        assert!(!update.appended);
        assert!(update.content.contains("- new"));
        assert!(!update.content.contains("- old"));
        assert!(update.content.contains("<!-- agent:queue go -->"));
        assert!(!update.content.contains("agent:queue auto"));
    }

    #[test]
    fn inactive_route_queue_head_honors_marker_controls_and_snapshot_backing() {
        let committed = concat!(
            "---\nagent_doc_format: template\nqueue_active: false\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#committed]\n",
            "<!-- /agent:queue -->\n"
        );
        let disk = concat!(
            "---\nagent_doc_format: template\nqueue_active: false\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#fresh]\n",
            "- do [#committed]\n",
            "<!-- /agent:queue -->\n"
        );
        assert_eq!(
            inactive_route_queue_head(disk, Some(false), Some(committed)).unwrap(),
            RouteInactiveQueueHead::Uncommitted("do [#fresh]".to_string())
        );
        assert_eq!(
            inactive_route_queue_head(committed, Some(false), Some(committed)).unwrap(),
            RouteInactiveQueueHead::Dispatchable("do [#committed]".to_string())
        );
        let stopped = committed.replace("agent:queue auto", "agent:queue auto stop");
        assert_eq!(
            inactive_route_queue_head(&stopped, Some(false), Some(&stopped)).unwrap(),
            RouteInactiveQueueHead::None
        );
    }

    #[test]
    fn committed_snapshot_backing_is_conservative() {
        let committed = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:queue -->\n",
            "- do [#x]\n",
            "- ~do [#done]~\n",
            "<!-- /agent:queue -->\n"
        );
        assert!(committed_snapshot_backs_queue_head(None, "anything"));
        assert!(committed_snapshot_backs_queue_head(
            Some(committed),
            "do [#x]"
        ));
        assert!(committed_snapshot_backs_queue_head(
            Some(committed),
            "do [#done]"
        ));
        assert!(!committed_snapshot_backs_queue_head(
            Some(committed),
            "do [#fresh]"
        ));
        assert!(!committed_snapshot_backs_queue_head(
            Some("---\nagent_doc_format: template\n---\n"),
            "do [#fresh]"
        ));
    }

    #[test]
    fn activation_content_flips_queue_state_and_strips_auto() {
        let original = concat!(
            "---\nagent_doc_format: template\nqueue_active: false\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#x]\n",
            "<!-- /agent:queue -->\n"
        );
        let activated = activate_existing_route_queue_content(original).unwrap();
        assert!(activated.contains("queue: go"));
        assert!(activated.contains("<!-- agent:queue go -->"));
        assert!(!activated.contains("agent:queue auto"));
    }
}
