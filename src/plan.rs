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
//! - `test_plan_detects_backlog_request`
//! - `test_plan_detects_recommendation_request`
//! - `test_plan_no_false_positive_on_questions`

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::{
    component,
    component::{is_backlog_component, is_tracked_work_component},
    diff, frontmatter, pending, security,
};

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
    ExpectAdd,
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

    let Some(diff_text) =
        diff::compute(file)?.or(crate::harness_prompt::synthetic_diff_for_file(file)?)
    else {
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
    let added_diff_lines = crate::prompt_contract::collect_added_diff_lines(&diff_text);

    let repo_actions = diff::extract_imperative_directives(&diff_text);
    let orchestration_request = diff::detect_orchestration_request(&diff_text);
    let exchange_compaction_requested = diff::detect_exchange_compaction_request(&diff_text);
    let parsed_commands = diff::parse_slash_commands_classified(&diff_text);
    let pending_mutations =
        pending_mutations_for_doc(&content, &repo_actions, &prompt_targets, &added_diff_lines)?;
    let mut blockers = shared_doc_security_blockers(file, &fm, &pending_mutations);

    let mut required_commands = Vec::new();
    let mut handoff = HandoffTarget::None;

    if exchange_compaction_requested {
        required_commands.push(format!(
            "Run `agent-doc compact {} --commit` before any free-form response.",
            file.display()
        ));
        handoff = HandoffTarget::Compact;
    }

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

    if !exchange_compaction_requested {
        required_commands.extend(finalize_placeholder_commands(file, &fm));
    }

    Ok(DispatchPlan {
        prompt_targets,
        repo_actions,
        required_commands,
        pending_mutations,
        handoff,
        blockers: std::mem::take(&mut blockers),
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
    prompt_targets: &[String],
    added_diff_lines: &[String],
) -> Result<Vec<PendingMutationPlan>> {
    let (fm, _) = frontmatter::parse(content).context("failed to parse frontmatter")?;
    let components = component::parse(content).context("failed to parse document components")?;
    let has_backlog = components
        .iter()
        .any(|component| is_backlog_component(&component.name));

    if !has_backlog {
        return Ok(Vec::new());
    }

    let items: Vec<pending::PendingItem> = components
        .iter()
        .filter(|component| is_tracked_work_component(&component.name))
        .flat_map(|component| {
            let (_, items, _) = pending::parse_items(component.content(content));
            items
        })
        .collect();
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

    if crate::prompt_contract::prompt_requests_backlog_work(
        prompt_targets,
        added_diff_lines,
        &fm.prompt_presets,
    ) {
        pending_mutations.push(PendingMutationPlan {
            kind: PendingMutationKind::ExpectAdd,
            id: String::new(),
            text: "user requested backlog/recommendations".to_string(),
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

fn shared_doc_security_blockers(
    file: &Path,
    fm: &frontmatter::Frontmatter,
    pending_mutations: &[PendingMutationPlan],
) -> Vec<String> {
    if fm.collaboration_mode() != frontmatter::CollaborationMode::Shared || fm.has_security_review()
    {
        return Vec::new();
    }

    pending_mutations
        .iter()
        .filter(|mutation| mutation.kind == PendingMutationKind::ResolveExisting)
        .filter_map(|mutation| {
            let referenced = security::referenced_markdown_path(file, &mutation.text)?;
            Some(format!(
                "Shared document item `#{}` references {} but this file has no `agent_doc_security_review`. Add an approved review marker before reading another plan/backlog document in shared mode.",
                mutation.id,
                referenced.display()
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot;
    use tempfile::TempDir;

    struct EnvGuard {
        key: &'static str,
        prev: Option<String>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let lock = crate::harness_prompt::TEST_ENV_LOCK.lock().unwrap();
            let prev = std::env::var(key).ok();
            unsafe { std::env::set_var(key, value) };
            Self {
                key,
                prev,
                _lock: lock,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(value) = &self.prev {
                unsafe { std::env::set_var(self.key, value) };
            } else {
                unsafe { std::env::remove_var(self.key) };
            }
        }
    }

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

<!-- agent:pending -->
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

<!-- agent:pending -->
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

    #[test]
    fn build_plan_resolves_existing_icebox_item_for_do_directive() {
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

## Pending

<!-- agent:pending -->
<!-- /agent:pending -->

## Icebox

<!-- agent:icebox -->
- [ ] [#ice01] Parked follow-up
<!-- /agent:icebox -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
do #ice01. spec-test-build-install-commit-push
<!-- /agent:exchange -->

## Pending

<!-- agent:pending -->
<!-- /agent:pending -->

## Icebox

<!-- agent:icebox -->
- [ ] [#ice01] Parked follow-up
<!-- /agent:icebox -->
"#;

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        assert_eq!(plan.pending_mutations.len(), 1);
        assert_eq!(
            plan.pending_mutations[0].kind,
            PendingMutationKind::ResolveExisting
        );
        assert_eq!(plan.pending_mutations[0].id, "ice01");
    }

    #[test]
    fn build_plan_dispatches_compact_exchange_request() {
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

compact exchange
<!-- /agent:exchange -->
"#;

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        assert_eq!(plan.handoff, HandoffTarget::Compact);
        assert!(
            plan.required_commands
                .iter()
                .any(|cmd| cmd.contains("agent-doc compact") && cmd.contains("--commit")),
            "expected compact handoff command, got: {:?}",
            plan.required_commands
        );
        assert!(
            !plan
                .required_commands
                .iter()
                .any(|cmd| cmd.contains("agent-doc finalize")),
            "compact handoff should not advertise finalize: {:?}",
            plan.required_commands
        );
    }

    #[test]
    fn test_plan_detects_backlog_request() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Done.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#abc1] Existing item
<!-- /agent:backlog -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Done.

add to backlog: what tasks remain?
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#abc1] Existing item
<!-- /agent:backlog -->
"#;

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        let expect_add = plan
            .pending_mutations
            .iter()
            .find(|m| m.kind == PendingMutationKind::ExpectAdd);
        assert!(
            expect_add.is_some(),
            "expected ExpectAdd mutation, got: {:?}",
            plan.pending_mutations
        );
    }

    #[test]
    fn test_plan_detects_recommendation_request() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Done.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Done.

What should we do next? Any recommendations?
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->
"#;

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        let expect_add = plan
            .pending_mutations
            .iter()
            .find(|m| m.kind == PendingMutationKind::ExpectAdd);
        assert!(
            expect_add.is_some(),
            "expected ExpectAdd for recommendation request, got: {:?}",
            plan.pending_mutations
        );
    }

    #[test]
    fn test_plan_detects_backlog_request_via_prompt_preset_expansion() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
prompt_presets:
  '#code-review': Please review the codebase. '#follow-up-backlog'
  '#follow-up-backlog': Any follow-up items to place in the backlog?
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Done.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
prompt_presets:
  '#code-review': Please review the codebase. '#follow-up-backlog'
  '#follow-up-backlog': Any follow-up items to place in the backlog?
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Done.

❯ #code-review
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->
"#;

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        let expect_add = plan
            .pending_mutations
            .iter()
            .find(|m| m.kind == PendingMutationKind::ExpectAdd);
        assert!(
            expect_add.is_some(),
            "expected ExpectAdd for preset-expanded backlog request, got: {:?}",
            plan.pending_mutations
        );
    }

    #[test]
    fn test_plan_no_false_positive_on_questions() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Done.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#xyz1] Some item
<!-- /agent:backlog -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Done.

How does the CRDT merge work?
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#xyz1] Some item
<!-- /agent:backlog -->
"#;

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        let expect_add = plan
            .pending_mutations
            .iter()
            .find(|m| m.kind == PendingMutationKind::ExpectAdd);
        assert!(
            expect_add.is_none(),
            "should not emit ExpectAdd for a plain question, got: {:?}",
            plan.pending_mutations
        );
    }

    #[test]
    fn build_plan_uses_harness_prompt_when_snapshot_matches_document() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("plan.md");

        let content = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
prompt_presets:
  '#code-review': Please review the codebase. '#follow-up-backlog'
  '#follow-up-backlog': Any follow-up items to place in the backlog?
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Done.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->
"#;

        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let _prompt = EnvGuard::set(
            "AGENT_DOC_HARNESS_PROMPT",
            &format!("agent-doc {} #code-review", doc.display()),
        );
        let plan = build(&doc).unwrap();

        assert!(
            plan.blockers.is_empty(),
            "unexpected blockers: {:?}",
            plan.blockers
        );
        assert!(
            plan.pending_mutations
                .iter()
                .any(|m| m.kind == PendingMutationKind::ExpectAdd),
            "expected ExpectAdd from harness prompt preset expansion, got {:?}",
            plan.pending_mutations
        );
    }

    #[test]
    fn build_plan_blocks_shared_doc_plan_reference_without_security_review() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
agent_doc_collaboration: shared
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#spec2] Follow tasks/plan-follow-up.md before rollout
<!-- /agent:backlog -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
agent_doc_collaboration: shared
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.

do #spec2. spec-test-build-install-commit-push
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#spec2] Follow tasks/plan-follow-up.md before rollout
<!-- /agent:backlog -->
"#;

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();
        assert_eq!(plan.pending_mutations.len(), 1);
        assert_eq!(plan.pending_mutations[0].id, "spec2");
        assert_eq!(plan.blockers.len(), 1);
        assert!(plan.blockers[0].contains("agent_doc_security_review"));
    }

    #[test]
    fn build_plan_allows_shared_doc_plan_reference_with_security_review() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
agent_doc_collaboration: shared
agent_doc_security_review: sec-1
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#spec2] Follow tasks/plan-follow-up.md before rollout
<!-- /agent:backlog -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
agent_doc_collaboration: shared
agent_doc_security_review: sec-1
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.

do #spec2. spec-test-build-install-commit-push
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#spec2] Follow tasks/plan-follow-up.md before rollout
<!-- /agent:backlog -->
"#;

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();
        assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
        assert_eq!(plan.pending_mutations.len(), 1);
        assert_eq!(plan.pending_mutations[0].id, "spec2");
    }

    #[test]
    fn build_plan_resolves_existing_pending_item_from_harness_prompt() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("plan.md");

        let content = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Done.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#1g42] Add the post-preflight dispatch phase
<!-- /agent:backlog -->
"#;

        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let _prompt = EnvGuard::set(
            "AGENT_DOC_HARNESS_PROMPT",
            &format!(
                "agent-doc {}\ndo #1g42. spec-test-build-install-commit-push",
                doc.display()
            ),
        );
        let plan = build(&doc).unwrap();

        assert!(
            plan.blockers.is_empty(),
            "unexpected blockers: {:?}",
            plan.blockers
        );
        assert!(
            plan.pending_mutations
                .iter()
                .any(|m| { m.kind == PendingMutationKind::ResolveExisting && m.id == "1g42" }),
            "expected ResolveExisting for harness prompt do-directive, got {:?}",
            plan.pending_mutations
        );
    }
}
