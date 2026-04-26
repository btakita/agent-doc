//! # Module: plan
//!
//! ## Spec
//! - `run(file)`: derives a structured planning/dispatch record for the current
//!   session document and prints it as pretty JSON.
//! - Reads the current document and computes the current diff against the saved
//!   snapshot via `diff::compute(file)`.
//! - Produces an ordered record with prompt targets, repo actions, required
//!   binary commands, pending mutations that must be resolved this cycle,
//!   a handoff target, and concrete blockers.
//! - Uses the same deterministic diff classifiers as preflight (`prompt_bearing_changes`,
//!   imperative-directive extraction, slash-command parsing, orchestration detection)
//!   so the planning record is binary-owned rather than free-form skill prose.
//! - `build(file)`: pure planning entry point for tests/callers after the file
//!   read + diff computation; returns the structured plan instead of printing.
//!
//! ## Agentic Contracts
//! - The plan record is deterministic for a given document + snapshot pair.
//! - `handoff=orchestrate` means the skill should execute the emitted
//!   `agent-doc orchestrate ...` command before attempting a manual response.
//! - `pending_mutations` captures pre-response pending work that must be
//!   explicitly resolved before persistence; it does not silently complete items.
//! - `required_commands` may include placeholder arguments such as
//!   `<preflight.baseline_file>` because the planning phase does not own the
//!   preflight baseline path.
//!
//! ## Evals
//! - `build_plan_detects_orchestration_handoff_and_existing_pending_item`
//! - `build_plan_includes_finalize_placeholder_for_template_docs`

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::{component, component::is_backlog_component, diff, frontmatter, pending};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct DispatchPlan {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prompt_targets: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repo_actions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_commands: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_mutations: Vec<PendingMutationPlan>,
    pub handoff: HandoffTarget,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blockers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PendingMutationPlan {
    pub kind: PendingMutationKind,
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingMutationKind {
    ResolveExisting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffTarget {
    None,
    Orchestrate,
    Compact,
    Claim,
    Other,
}

pub fn run(file: &Path) -> Result<()> {
    let plan = build(file)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&plan).context("failed to serialize dispatch plan")?
    );
    Ok(())
}

pub fn build(file: &Path) -> Result<DispatchPlan> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let (fm, _body) = frontmatter::parse(&content)
        .with_context(|| format!("failed to parse frontmatter in {}", file.display()))?;

    let Some(diff_text) = diff::compute(file)? else {
        return Ok(DispatchPlan {
            prompt_targets: Vec::new(),
            repo_actions: Vec::new(),
            required_commands: finalize_placeholder_commands(file, &fm),
            pending_mutations: Vec::new(),
            handoff: HandoffTarget::None,
            blockers: vec!["No changes detected since the last snapshot.".to_string()],
        });
    };

    let prompt_targets = diff::classify_prompt_bearing_changes(&diff_text)
        .into_iter()
        .filter(|change| change.kind == diff::PromptBearingChangeKind::PromptTarget)
        .map(|change| change.text)
        .collect::<Vec<_>>();

    let repo_actions = diff::extract_imperative_directives(&diff_text);
    let orchestration_request = diff::detect_orchestration_request(&diff_text);
    let parsed_commands = diff::parse_slash_commands_classified(&diff_text);
    let pending_mutations = pending_mutations_for_doc(&content, &repo_actions)?;

    let mut required_commands = Vec::new();
    let mut handoff = HandoffTarget::None;

    if let Some(request) = orchestration_request {
        required_commands.push(format!(
            "agent-doc orchestrate {} --mode {} --from-exchange",
            file.display(),
            orchestration_mode_arg(request.mode)
        ));
        handoff = HandoffTarget::Orchestrate;
    }

    for command in parsed_commands.builtin_commands {
        match command.as_str() {
            "/compact" => {
                handoff = HandoffTarget::Compact;
                required_commands.push(format!(
                    "Tell the user to run `{}` at the terminal before continuing.",
                    command
                ));
            }
            _ => {
                required_commands.push(format!(
                    "Tell the user to run `{}` at the terminal before continuing.",
                    command
                ));
                if matches!(handoff, HandoffTarget::None) {
                    handoff = HandoffTarget::Other;
                }
            }
        }
    }

    for command in parsed_commands.skill_commands {
        required_commands.push(format!(
            "Dispatch slash command before free-form reply: {}",
            command
        ));
        if matches!(handoff, HandoffTarget::None) {
            handoff = HandoffTarget::Other;
        }
    }

    required_commands.extend(finalize_placeholder_commands(file, &fm));

    Ok(DispatchPlan {
        prompt_targets,
        repo_actions,
        required_commands,
        pending_mutations,
        handoff,
        blockers: Vec::new(),
    })
}

fn orchestration_mode_arg(mode: diff::OrchestrationRequestMode) -> &'static str {
    match mode {
        diff::OrchestrationRequestMode::Sequential => "sequential",
        diff::OrchestrationRequestMode::Parallel => "parallel",
        diff::OrchestrationRequestMode::Dag => "dag",
    }
}

fn finalize_placeholder_commands(file: &Path, fm: &frontmatter::Frontmatter) -> Vec<String> {
    let mut finalize = format!(
        "agent-doc finalize {} --baseline-file <preflight.baseline_file> --origin skill",
        file.display()
    );
    if fm.resolve_mode().is_crdt() {
        finalize.push_str(" --stream");
    } else if fm.resolve_mode().is_template() {
        finalize.push_str(" --template");
    }

    vec![finalize]
}

fn pending_mutations_for_doc(
    content: &str,
    repo_actions: &[String],
) -> Result<Vec<PendingMutationPlan>> {
    let components = component::parse(content).context("failed to parse document components")?;
    let Some(pending_component) = components
        .iter()
        .find(|component| is_backlog_component(&component.name))
    else {
        return Ok(Vec::new());
    };

    let (_, items, _) = pending::parse_items(pending_component.content(content));
    let mut pending_mutations: Vec<PendingMutationPlan> = Vec::new();

    for action in repo_actions {
        let Some(id) = extract_do_pending_id(action) else {
            continue;
        };
        let Some(item) = items.iter().find(|item| {
            item.id.eq_ignore_ascii_case(&id) && item.state != pending::PendingState::Done
        }) else {
            continue;
        };
        if pending_mutations
            .iter()
            .any(|mutation| mutation.id == item.id)
        {
            continue;
        }
        pending_mutations.push(PendingMutationPlan {
            kind: PendingMutationKind::ResolveExisting,
            id: item.id.clone(),
            text: item.text.clone(),
        });
    }

    Ok(pending_mutations)
}

fn extract_do_pending_id(action: &str) -> Option<String> {
    let lower = action.to_ascii_lowercase();
    let rest = lower.strip_prefix("do ")?;
    let hash_idx = rest.find('#')?;
    let id_start = hash_idx + 1;
    let id: String = rest[id_start..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    (!id.is_empty()).then_some(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot;
    use tempfile::TempDir;

    #[test]
    fn build_plan_detects_orchestration_handoff_and_existing_pending_item() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->

## Pending

<!-- agent:pending patch=replace -->
- [ ] [#1g42] Add the post-preflight dispatch phase
<!-- /agent:pending -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.

synchronous orcestra
- do #1g42 Option A. update spec + tests. build + install for local testing. commit + push
- do #1g42 Option B. update spec + tests. build + install for local testing. commit + push
<!-- /agent:exchange -->

## Pending

<!-- agent:pending patch=replace -->
- [ ] [#1g42] Add the post-preflight dispatch phase
<!-- /agent:pending -->
"#;

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        assert_eq!(plan.handoff, HandoffTarget::Orchestrate);
        assert!(
            plan.required_commands
                .iter()
                .any(|cmd| cmd.contains("agent-doc orchestrate")
                    && cmd.contains("--mode sequential")),
            "expected orchestrate handoff command, got: {:?}",
            plan.required_commands
        );
        assert_eq!(plan.repo_actions.len(), 2);
        assert_eq!(plan.pending_mutations.len(), 1);
        assert_eq!(plan.pending_mutations[0].id, "1g42");
        assert_eq!(
            plan.pending_mutations[0].kind,
            PendingMutationKind::ResolveExisting
        );
        assert_eq!(plan.prompt_targets.len(), 2);
    }

    #[test]
    fn build_plan_includes_finalize_placeholder_for_template_docs() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
What changed?
<!-- /agent:exchange -->
"#;

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        assert!(
            plan.required_commands.iter().any(|cmd| {
                cmd.contains("agent-doc finalize")
                    && cmd.contains("--baseline-file <preflight.baseline_file>")
                    && cmd.contains("--stream")
            }),
            "expected finalize placeholder command, got: {:?}",
            plan.required_commands
        );
        assert_eq!(plan.handoff, HandoffTarget::None);
        assert!(plan.blockers.is_empty());
    }
}
