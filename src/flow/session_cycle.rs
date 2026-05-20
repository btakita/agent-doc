use super::types::{FlowEvent, FlowName, FlowOutcome, FlowStage};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionCycleStep {
    pub(crate) stage: FlowStage,
    pub(crate) outcome: FlowOutcome,
    pub(crate) reason: Option<String>,
}

impl SessionCycleStep {
    pub(crate) fn completed(stage: FlowStage) -> Self {
        Self {
            stage,
            outcome: FlowOutcome::Completed,
            reason: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionExecutionScope {
    Normal,
    PlanBacklogOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FinalizePendingMutationKind {
    ResolveExisting,
    ExpectAdd,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct FinalizePendingMutation<'a> {
    pub(crate) kind: FinalizePendingMutationKind,
    pub(crate) id: &'a str,
    pub(crate) target_files: &'a [String],
}

pub(crate) fn prompt_targets_from_changes(
    changes: &[crate::diff::PromptBearingChange],
) -> Vec<String> {
    changes
        .iter()
        .filter(|change| change.kind == crate::diff::PromptBearingChangeKind::PromptTarget)
        .map(|change| change.text.clone())
        .collect()
}

pub(crate) fn classify_execution_scope(
    prompt_targets: &[String],
    added_diff_lines: &[String],
    harness_prompt_only: bool,
    prompt_presets: &indexmap::IndexMap<String, String>,
) -> SessionExecutionScope {
    let agent_doc_bug_requested = crate::prompt_contract::prompt_targets_reference_preset(
        prompt_targets,
        prompt_presets,
        "#agent-doc-bug",
    ) || harness_prompt_only
        && crate::prompt_contract::prompt_targets_reference_preset(
            added_diff_lines,
            prompt_presets,
            "#agent-doc-bug",
        );
    if agent_doc_bug_requested {
        SessionExecutionScope::PlanBacklogOnly
    } else {
        SessionExecutionScope::Normal
    }
}

pub(crate) fn finalize_command(
    file: &Path,
    mode: crate::frontmatter::ResolvedMode,
    pending_mutations: &[FinalizePendingMutation<'_>],
) -> String {
    let mut finalize = format!(
        "agent-doc finalize {} --baseline-file <preflight.baseline_file> --origin skill",
        file.display()
    );
    for mutation in pending_mutations
        .iter()
        .filter(|mutation| mutation.kind == FinalizePendingMutationKind::ResolveExisting)
    {
        finalize.push_str(" --done ");
        finalize.push_str(mutation.id);
    }
    for mutation in pending_mutations
        .iter()
        .filter(|mutation| mutation.kind == FinalizePendingMutationKind::ExpectAdd)
    {
        for target in mutation.target_files {
            finalize.push_str(" --pending-add-to ");
            finalize.push_str(target);
            finalize.push_str(" \"<item>\"");
        }
    }
    if mode.is_crdt() {
        finalize.push_str(" --stream");
    } else if mode.is_template() {
        finalize.push_str(" --template");
    }
    finalize
}

pub(crate) fn session_cycle_event(
    stage: FlowStage,
    outcome: FlowOutcome,
    reason: impl Into<Option<String>>,
) -> FlowEvent {
    let mut event = FlowEvent::new(FlowName::SessionCycle, stage, outcome);
    if let Some(reason) = reason.into() {
        event = event.with_reason(reason);
    }
    event
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prompt_change(text: &str) -> crate::diff::PromptBearingChange {
        crate::diff::PromptBearingChange {
            kind: crate::diff::PromptBearingChangeKind::PromptTarget,
            text: text.to_string(),
        }
    }

    #[test]
    fn prompt_targets_keep_prompt_target_order() {
        let changes = vec![prompt_change("first"), prompt_change("second")];

        assert_eq!(
            prompt_targets_from_changes(&changes),
            vec!["first".to_string(), "second".to_string()]
        );
    }

    #[test]
    fn classify_execution_scope_keeps_agent_doc_bug_plan_only() {
        let mut presets = indexmap::IndexMap::new();
        presets.insert(
            "#agent-doc-bug".to_string(),
            "Please create a plan.".to_string(),
        );

        let scope = classify_execution_scope(
            &["#agent-doc-bug\nBroken flow".to_string()],
            &[],
            false,
            &presets,
        );

        assert_eq!(scope, SessionExecutionScope::PlanBacklogOnly);
    }

    #[test]
    fn finalize_command_includes_done_flags_and_stream_mode() {
        let targets = vec!["tasks/bugs.md".to_string()];
        let pending = vec![
            FinalizePendingMutation {
                kind: FinalizePendingMutationKind::ResolveExisting,
                id: "abc1",
                target_files: &[],
            },
            FinalizePendingMutation {
                kind: FinalizePendingMutationKind::ExpectAdd,
                id: "",
                target_files: &targets,
            },
        ];
        let mode = crate::frontmatter::ResolvedMode {
            format: crate::frontmatter::AgentDocFormat::Template,
            write: crate::frontmatter::AgentDocWrite::Crdt,
        };

        let command = finalize_command(Path::new("tasks/doc.md"), mode, &pending);

        assert!(command.contains("--done abc1"));
        assert!(command.contains("--pending-add-to tasks/bugs.md \"<item>\""));
        assert!(command.ends_with(" --stream"));
    }
}
