use crate::CyclePhase;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CycleAckState<'a> {
    pub cycle_id: &'a str,
    pub phase: CyclePhase,
    pub updated_at: u64,
    pub last_event: &'a str,
}

impl CycleAckState<'_> {
    pub const fn is_open(self) -> bool {
        !matches!(self.phase, CyclePhase::Committed | CyclePhase::Abandoned)
    }
}

pub fn cycle_state_advances_start_ack(
    current: CycleAckState<'_>,
    baseline: Option<CycleAckState<'_>>,
) -> bool {
    match baseline {
        None => true,
        Some(previous) if previous.is_open() => {
            current.cycle_id != previous.cycle_id
                || current.updated_at != previous.updated_at
                || current.phase != previous.phase
                || current.last_event != previous.last_event
        }
        Some(previous) => current.cycle_id != previous.cycle_id,
    }
}

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
    fn ack_advances_without_baseline() {
        assert!(cycle_state_advances_start_ack(
            CycleAckState {
                cycle_id: "cycle-1",
                phase: CyclePhase::PreflightStarted,
                updated_at: 1,
                last_event: "start_preflight",
            },
            None,
        ));
    }

    #[test]
    fn ack_advances_open_baseline_on_same_cycle_mutation() {
        let baseline = CycleAckState {
            cycle_id: "cycle-1",
            phase: CyclePhase::PreflightStarted,
            updated_at: 1,
            last_event: "start_preflight",
        };
        let current = CycleAckState {
            updated_at: 2,
            last_event: "turn_checkpoint",
            ..baseline
        };
        assert!(cycle_state_advances_start_ack(current, Some(baseline)));
    }

    #[test]
    fn ack_ignores_closed_baseline_same_cycle_mutation() {
        let baseline = CycleAckState {
            cycle_id: "cycle-1",
            phase: CyclePhase::Committed,
            updated_at: 1,
            last_event: "commit_success",
        };
        let current = CycleAckState {
            updated_at: 2,
            last_event: "commit_already_current",
            ..baseline
        };
        assert!(!cycle_state_advances_start_ack(current, Some(baseline)));
    }

    #[test]
    fn ack_advances_closed_baseline_only_for_new_cycle() {
        let baseline = CycleAckState {
            cycle_id: "cycle-1",
            phase: CyclePhase::Committed,
            updated_at: 1,
            last_event: "commit_success",
        };
        let current = CycleAckState {
            cycle_id: "cycle-2",
            phase: CyclePhase::PreflightStarted,
            updated_at: 2,
            last_event: "start_preflight",
        };
        assert!(cycle_state_advances_start_ack(current, Some(baseline)));
    }

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
