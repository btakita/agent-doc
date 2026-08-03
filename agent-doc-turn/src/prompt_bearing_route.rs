#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptBearingRouteContext {
    pub marker: String,
    pub prompt_text: String,
    pub slash_command: Option<String>,
}

pub fn prompt_bearing_route_context_from_change(
    change: &agent_doc_diff::PromptBearingChange,
) -> Option<PromptBearingRouteContext> {
    let marker = match change.kind {
        agent_doc_diff::PromptBearingChangeKind::PromptTarget => "prompt_target",
        agent_doc_diff::PromptBearingChangeKind::ContentEdit => "content_edit",
        agent_doc_diff::PromptBearingChangeKind::RecoveryArtifact
        | agent_doc_diff::PromptBearingChangeKind::BoundaryArtifact => return None,
    };
    let preview = change
        .text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(change.text.as_str())
        .trim();
    let prompt_text = agent_doc_queue::route_dispatch::route_prompt_text_for_change(&change.text)
        .unwrap_or_else(|| preview.trim_start_matches('❯').trim().to_string());
    let slash_command = agent_doc_queue::queue_command::slash_command_text(&prompt_text);
    Some(PromptBearingRouteContext {
        marker: format!("{marker}: {preview}"),
        prompt_text,
        slash_command,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_context_keeps_slash_command_literal() {
        let context =
            prompt_bearing_route_context_from_change(&agent_doc_diff::PromptBearingChange {
                kind: agent_doc_diff::PromptBearingChangeKind::PromptTarget,
                text: "❯ /clear\n<!-- agent:boundary:head -->".to_string(),
            })
            .expect("route context");

        assert_eq!(context.marker, "prompt_target: ❯ /clear");
        assert_eq!(context.prompt_text, "/clear");
        assert_eq!(context.slash_command.as_deref(), Some("/clear"));
    }

    #[test]
    fn prompt_context_ignores_recovery_and_boundary_artifacts() {
        for kind in [
            agent_doc_diff::PromptBearingChangeKind::RecoveryArtifact,
            agent_doc_diff::PromptBearingChangeKind::BoundaryArtifact,
        ] {
            let context =
                prompt_bearing_route_context_from_change(&agent_doc_diff::PromptBearingChange {
                    kind,
                    text: "<!-- agent:boundary:stale -->".to_string(),
                });
            assert!(context.is_none());
        }
    }
}
