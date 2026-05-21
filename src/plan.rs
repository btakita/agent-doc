//! # Module: plan
//!
//! ## Spec
//! - `run(file)`: derives a structured planning/dispatch record for the current
//!   session document and prints it as pretty JSON.
//! - Reads the current document and computes the current diff against the saved
//!   snapshot via `diff::compute(file)`.
//! - Produces an ordered record with prompt targets, execution scope, repo
//!   actions, required binary commands, pending mutations that must be
//!   resolved this cycle, a handoff target, and concrete blockers.
//! - Does not fail closed solely on session-accretion heuristics; accretion
//!   remains advisory while planning still derives prompt targets and actions.
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
//! - `execution_scope=plan_backlog_only` means the prompt contract is a
//!   report/planning turn such as `#agent-doc-bug`, so repo work must wait
//!   for a later explicit implementation directive.
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph_evidence: Option<crate::tsift_graph::TsiftGraphEvidencePlan>,
    pub dispatch_candidate: bool,
    pub task_class: String,
    pub risk: String,
    pub parallelizable: bool,
    pub suggested_parent_tier: String,
    pub model_tier: String,
    pub dispatch_mode: String,
    pub context_budget_tokens: usize,
    pub job_packet_budget_tokens: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub write_scope: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_proof: Vec<String>,
    pub tsift_context: TsiftContextPlan,
    pub execution_scope: ExecutionScope,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TsiftContextPlan {
    pub status: String,
    pub freshness_policy: String,
    pub index_status_command: String,
    pub context_pack_command: String,
    pub source_read_command: String,
    pub diff_digest_command: String,
    pub test_digest_command: String,
    pub stale_fallback: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionScope {
    Normal,
    PlanBacklogOnly,
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
    let (fm, _body) = frontmatter::parse_for_file(&content, file)
        .with_context(|| format!("failed to parse frontmatter in {}", file.display()))?;

    let doc_diff = diff::compute(file)?;
    let harness_diff = if doc_diff.is_none() {
        crate::harness_prompt::synthetic_diff_for_file(file)?
    } else {
        None
    };
    let queue_diff = if doc_diff.is_none() && harness_diff.is_none() {
        active_queue_prompt_diff(&content)
    } else {
        None
    };

    let Some(diff_text) = doc_diff.or(harness_diff.clone()).or(queue_diff) else {
        return Ok(DispatchPlan {
            prompt_targets: Vec::new(),
            graph_evidence: None,
            dispatch_candidate: false,
            task_class: "no_changes".to_string(),
            risk: "low".to_string(),
            parallelizable: false,
            suggested_parent_tier: "low".to_string(),
            model_tier: "low".to_string(),
            dispatch_mode: dispatch_mode(&fm),
            context_budget_tokens: 0,
            job_packet_budget_tokens: 0,
            write_scope: Vec::new(),
            required_proof: Vec::new(),
            tsift_context: tsift_context_plan(file),
            execution_scope: ExecutionScope::Normal,
            repo_actions: Vec::new(),
            required_commands: finalize_placeholder_commands(file, &fm, &[]),
            pending_mutations: Vec::new(),
            handoff: HandoffTarget::None,
            blockers: vec!["No changes detected since the last snapshot.".to_string()],
        });
    };

    let prompt_bearing_changes = diff::classify_prompt_bearing_changes(&diff_text);
    let prompt_targets =
        crate::flow::session_cycle::prompt_targets_from_changes(&prompt_bearing_changes);
    let added_diff_lines = crate::prompt_contract::collect_added_diff_lines(&diff_text);

    let execution_scope = execution_scope_for_prompt_targets(
        &prompt_targets,
        &added_diff_lines,
        harness_diff.is_some(),
        &fm.prompt_presets,
    );
    let repo_actions = if execution_scope == ExecutionScope::PlanBacklogOnly {
        Vec::new()
    } else {
        diff::extract_imperative_directives(&diff_text)
    };
    let orchestration_request = diff::detect_orchestration_request(&diff_text);
    let exchange_compaction_requested = diff::detect_exchange_compaction_request(&diff_text);
    let parsed_commands = diff::parse_slash_commands_classified(&diff_text);
    let pending_mutations = pending_mutations_for_doc(
        file,
        &content,
        &repo_actions,
        &prompt_targets,
        &added_diff_lines,
        &prompt_bearing_changes,
    )?;
    let routing = routing_fields(
        file,
        execution_scope,
        &repo_actions,
        &prompt_targets,
        &pending_mutations,
        &fm,
    );
    let mut blockers = shared_doc_security_blockers(file, &fm, &pending_mutations);
    let graph_targets = prompt_targets
        .iter()
        .chain(repo_actions.iter())
        .cloned()
        .collect::<Vec<_>>();
    let graph_evidence = match crate::tsift_graph::collect_for_do_items(file, &graph_targets) {
        Ok(graph_evidence) => graph_evidence,
        Err(err) => {
            blockers.push(format!("tsift graph evidence failed closed: {err:#}"));
            None
        }
    };

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

    if !exchange_compaction_requested && matches!(handoff, HandoffTarget::None) {
        required_commands.extend(finalize_placeholder_commands(file, &fm, &pending_mutations));
    }

    Ok(DispatchPlan {
        prompt_targets,
        graph_evidence,
        dispatch_candidate: routing.dispatch_candidate,
        task_class: routing.task_class,
        risk: routing.risk,
        parallelizable: routing.parallelizable,
        suggested_parent_tier: routing.suggested_parent_tier.clone(),
        model_tier: routing.model_tier,
        dispatch_mode: routing.dispatch_mode,
        context_budget_tokens: routing.context_budget_tokens,
        job_packet_budget_tokens: routing.job_packet_budget_tokens,
        write_scope: routing.write_scope,
        required_proof: routing.required_proof,
        tsift_context: routing.tsift_context,
        execution_scope,
        repo_actions,
        required_commands,
        pending_mutations,
        handoff,
        blockers: std::mem::take(&mut blockers),
    })
}

fn active_queue_prompt_diff(content: &str) -> Option<String> {
    let components = component::parse(content).ok()?;
    let queue_component = components
        .iter()
        .find(|component| component.name == "queue")?;
    let body = &content[queue_component.open_end..queue_component.close_start];
    let entries = crate::queue::parse(body).ok()?;
    let has_auto = crate::queue::has_auto_attr(&queue_component.attrs);
    let (fm, _) = frontmatter::parse(content).ok()?;
    let activation = crate::queue::resolve_activation(
        &entries,
        has_auto,
        false,
        fm.queue_active.unwrap_or(false),
    );
    if !activation.active {
        return None;
    }
    crate::queue::prompts(&activation.entries_after)
        .first()
        .map(|prompt| diff::synthetic_added_lines_diff(&prompt.text, "queue"))
}

fn orchestration_mode_arg(mode: diff::OrchestrationRequestMode) -> &'static str {
    match mode {
        diff::OrchestrationRequestMode::Sequential => "sequential",
        diff::OrchestrationRequestMode::Parallel => "parallel",
        diff::OrchestrationRequestMode::Dag => "dag",
    }
}

fn execution_scope_for_prompt_targets(
    prompt_targets: &[String],
    added_diff_lines: &[String],
    harness_prompt_only: bool,
    prompt_presets: &indexmap::IndexMap<String, String>,
) -> ExecutionScope {
    match crate::flow::session_cycle::classify_execution_scope(
        prompt_targets,
        added_diff_lines,
        harness_prompt_only,
        prompt_presets,
    ) {
        crate::flow::session_cycle::SessionExecutionScope::PlanBacklogOnly => {
            ExecutionScope::PlanBacklogOnly
        }
        crate::flow::session_cycle::SessionExecutionScope::Normal => ExecutionScope::Normal,
    }
}

fn finalize_placeholder_commands(
    file: &Path,
    fm: &frontmatter::Frontmatter,
    pending_mutations: &[PendingMutationPlan],
) -> Vec<String> {
    let pending = pending_mutations
        .iter()
        .map(
            |mutation| crate::flow::session_cycle::FinalizePendingMutation {
                kind: match mutation.kind {
                    PendingMutationKind::ResolveExisting => {
                        crate::flow::session_cycle::FinalizePendingMutationKind::ResolveExisting
                    }
                    PendingMutationKind::ExpectAdd => {
                        crate::flow::session_cycle::FinalizePendingMutationKind::ExpectAdd
                    }
                },
                id: &mutation.id,
                target_files: &mutation.target_files,
            },
        )
        .collect::<Vec<_>>();
    vec![crate::flow::session_cycle::finalize_command(
        file,
        fm.resolve_mode(),
        &pending,
    )]
}

struct RoutingFields {
    dispatch_candidate: bool,
    task_class: String,
    risk: String,
    parallelizable: bool,
    suggested_parent_tier: String,
    model_tier: String,
    dispatch_mode: String,
    context_budget_tokens: usize,
    job_packet_budget_tokens: usize,
    write_scope: Vec<String>,
    required_proof: Vec<String>,
    tsift_context: TsiftContextPlan,
}

fn routing_fields(
    file: &Path,
    execution_scope: ExecutionScope,
    repo_actions: &[String],
    prompt_targets: &[String],
    pending_mutations: &[PendingMutationPlan],
    fm: &frontmatter::Frontmatter,
) -> RoutingFields {
    let combined_text = prompt_targets
        .iter()
        .chain(repo_actions.iter())
        .chain(pending_mutations.iter().map(|mutation| &mutation.text))
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\n")
        .to_ascii_lowercase();
    let task_class = infer_task_class(repo_actions, &combined_text);
    let risk = infer_risk(repo_actions, &combined_text);
    let suggested_parent_tier = tier_for_risk(&risk).to_string();
    let dispatch_candidate = execution_scope == ExecutionScope::Normal
        && !repo_actions.is_empty()
        && dispatch_mode(fm) != "off";
    let parallelizable = dispatch_candidate && repo_actions.len() > 1;
    let context_budget_tokens = if risk == "high" { 10_000 } else { 6_000 };
    let mut required_proof = vec![
        "changed_paths".to_string(),
        "commands".to_string(),
        "verification".to_string(),
        "confidence".to_string(),
        "escalation".to_string(),
    ];
    if combined_text.contains("commit") {
        required_proof.push("commit".to_string());
    }
    if combined_text.contains("push") {
        required_proof.push("push".to_string());
    }

    RoutingFields {
        dispatch_candidate,
        task_class,
        risk: risk.clone(),
        parallelizable,
        suggested_parent_tier: suggested_parent_tier.clone(),
        model_tier: suggested_parent_tier,
        dispatch_mode: dispatch_mode(fm),
        context_budget_tokens,
        job_packet_budget_tokens: context_budget_tokens,
        write_scope: infer_write_scope(&combined_text),
        required_proof,
        tsift_context: tsift_context_plan(file),
    }
}

fn dispatch_mode(fm: &frontmatter::Frontmatter) -> String {
    fm.dispatch
        .as_deref()
        .unwrap_or("manual")
        .trim()
        .to_ascii_lowercase()
}

fn infer_task_class(repo_actions: &[String], text: &str) -> String {
    if repo_actions.is_empty() {
        return "prompt_response".to_string();
    }
    if text.contains("lower-agent") || text.contains("orchestration") || text.contains("job packet")
    {
        return "lower_agent_orchestration".to_string();
    }
    if text.contains("spec") && text.contains("test") {
        return "spec_test_build".to_string();
    }
    if repo_actions.len() > 1 {
        return "multi_target_repo_work".to_string();
    }
    "tracked_repo_work".to_string()
}

fn infer_risk(repo_actions: &[String], text: &str) -> String {
    let high_risk_terms = [
        "concurrency",
        "crdt",
        "tmux",
        "routing",
        "orchestration",
        "lower-agent",
        "job packet",
        "dispatch",
        "git",
        "security",
        "cross-module",
    ];
    if repo_actions.len() > 3 || high_risk_terms.iter().any(|term| text.contains(term)) {
        return "high".to_string();
    }
    if repo_actions.len() > 1 || text.contains("test") || text.contains("build") {
        return "medium".to_string();
    }
    "low".to_string()
}

fn tier_for_risk(risk: &str) -> &'static str {
    match risk {
        "high" => "high",
        "medium" => "med",
        _ => "low",
    }
}

fn infer_write_scope(text: &str) -> Vec<String> {
    let mut scope = Vec::new();
    if text.contains("agent-doc") || text.contains("job") || text.contains("plan") {
        scope.push("src/agent-doc/src/".to_string());
    }
    if text.contains("spec") {
        scope.push("src/agent-doc/specs/".to_string());
    }
    if text.contains("runbook") {
        scope.push("src/agent-doc/runbooks/".to_string());
    }
    if text.contains("test") {
        scope.push("src/agent-doc/tests/".to_string());
    }
    if text.contains("tsift") {
        scope.push("src/tsift/".to_string());
    }
    scope.sort();
    scope.dedup();
    if scope.is_empty() {
        scope.push("undetermined".to_string());
    }
    scope
}

fn tsift_context_plan(file: &Path) -> TsiftContextPlan {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let root = crate::snapshot::find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    let status = if root.join(".tsift/index.db").exists() {
        "available"
    } else if root.join(".tsift").is_dir() {
        "missing_index"
    } else {
        "missing"
    };
    let file_arg = shell_quote(&display_path(file));
    let root_arg = shell_quote(&display_path(&root));
    TsiftContextPlan {
        status: status.to_string(),
        freshness_policy: "fresh context required for automatic dispatch; manual packets record stale or missing context explicitly".to_string(),
        index_status_command: format!("tsift status --json {root_arg}"),
        context_pack_command: format!("tsift context-pack {file_arg} --json --budget normal"),
        source_read_command: "tsift source-read <handle>".to_string(),
        diff_digest_command: format!("tsift diff-digest --cached {root_arg} --json"),
        test_digest_command: format!("tsift test-digest --path {root_arg} --input <test.log> --json"),
        stale_fallback: "fail closed before automatic lower-agent dispatch; for manual packets, include the diagnostic and require parent review".to_string(),
    }
}

fn display_path(path: &Path) -> String {
    path.display().to_string()
}

fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-' | b':')
        })
    {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn pending_mutations_for_doc(
    file: &Path,
    content: &str,
    repo_actions: &[String],
    prompt_targets: &[String],
    added_diff_lines: &[String],
    prompt_bearing_changes: &[diff::PromptBearingChange],
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
        for id in extract_do_pending_ids(action) {
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
                target_files: Vec::new(),
            });
        }
    }

    if crate::prompt_contract::prompt_requests_backlog_work(
        prompt_targets,
        added_diff_lines,
        &fm.prompt_presets,
    ) {
        let target_files = crate::prompt_contract::explicit_backlog_targets(
            file,
            prompt_targets,
            added_diff_lines,
            &fm.prompt_presets,
        )?
        .into_iter()
        .map(|path| path.display().to_string())
        .collect();
        let issue_units = crate::prompt_contract::ordered_issue_units_for_agent_doc_bug(
            prompt_targets,
            added_diff_lines,
            &fm.prompt_presets,
            prompt_bearing_changes,
        );
        if issue_units.len() > 1 {
            eprintln!(
                "[plan] #agent-doc-bug declaration_order={} final_insert_order={}",
                issue_units
                    .iter()
                    .enumerate()
                    .map(|(idx, unit)| format!("{}:{}", idx + 1, truncate_for_plan_log(unit)))
                    .collect::<Vec<_>>()
                    .join(" | "),
                issue_units
                    .iter()
                    .enumerate()
                    .map(|(idx, unit)| format!("{}:{}", idx + 1, truncate_for_plan_log(unit)))
                    .collect::<Vec<_>>()
                    .join(" | ")
            );
        }
        if issue_units.is_empty() {
            pending_mutations.push(PendingMutationPlan {
                kind: PendingMutationKind::ExpectAdd,
                id: String::new(),
                text: "user requested backlog/recommendations".to_string(),
                target_files,
            });
        } else {
            for issue in issue_units {
                pending_mutations.push(PendingMutationPlan {
                    kind: PendingMutationKind::ExpectAdd,
                    id: String::new(),
                    text: issue,
                    target_files: target_files.clone(),
                });
            }
        }
    }

    Ok(pending_mutations)
}

fn truncate_for_plan_log(text: &str) -> String {
    const MAX: usize = 80;
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= MAX {
        return normalized;
    }
    let mut out = normalized.chars().take(MAX).collect::<String>();
    out.push_str("...");
    out
}

fn extract_do_pending_ids(action: &str) -> Vec<String> {
    crate::tsift_graph::extract_do_targets(action)
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
    use std::io::Write;
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

        fn unset(key: &'static str) -> Self {
            let lock = crate::harness_prompt::TEST_ENV_LOCK.lock().unwrap();
            let prev = std::env::var(key).ok();
            unsafe { std::env::remove_var(key) };
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

    fn write_cycles_log(doc: &std::path::Path, entries: &[crate::ops_log::CycleEntry]) {
        let log_path = doc.parent().unwrap().join(".agent-doc/logs/cycles.jsonl");
        std::fs::create_dir_all(log_path.parent().unwrap()).unwrap();
        let mut file = std::fs::File::create(log_path).unwrap();
        for entry in entries {
            writeln!(file, "{}", serde_json::to_string(entry).unwrap()).unwrap();
        }
    }

    fn setup_project() -> TempDir {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        dir
    }

    #[test]
    fn build_plan_detects_orchestration_handoff_and_existing_pending_item() {
        let dir = setup_project();
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
        let dir = setup_project();
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
    fn build_plan_uses_active_queue_prompt_when_document_has_no_diff() {
        let _prompt = EnvGuard::unset("AGENT_DOC_HARNESS_PROMPT");
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let content = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
queue_active: true
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->

<!-- agent:queue auto -->
- do [#oobpmt]
<!-- /agent:queue -->

<!-- agent:backlog -->
- [ ] [#oobpmt] Fix OOB prompt absorption.
<!-- /agent:backlog -->
"#;
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let plan = build(&doc).unwrap();

        assert!(
            plan.blockers.is_empty(),
            "active queue prompt should not plan as a no-op"
        );
        assert_eq!(plan.repo_actions, vec!["do [#oobpmt]"]);
        assert_eq!(plan.pending_mutations.len(), 1);
        assert_eq!(plan.pending_mutations[0].id, "oobpmt");
        assert!(
            plan.required_commands
                .iter()
                .any(|cmd| cmd.contains("--done oobpmt")),
            "queue do item should require closeout with --done oobpmt: {:?}",
            plan.required_commands
        );
    }

    #[test]
    fn build_plan_includes_pending_done_for_bracketed_do_directive() {
        let dir = setup_project();
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
- [ ] [#dodone] Close the matching backlog item
<!-- /agent:pending -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
do [#dodone]. spec-test-build-install-commit-push
<!-- /agent:exchange -->

## Pending

<!-- agent:pending -->
- [ ] [#dodone] Close the matching backlog item
<!-- /agent:pending -->
"#;

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        assert_eq!(plan.repo_actions.len(), 1);
        assert_eq!(plan.pending_mutations.len(), 1);
        assert_eq!(
            plan.pending_mutations[0].kind,
            PendingMutationKind::ResolveExisting
        );
        assert_eq!(plan.pending_mutations[0].id, "dodone");
        assert!(
            plan.required_commands
                .iter()
                .any(|cmd| cmd.contains("agent-doc finalize")
                    && cmd.contains("--done dodone")
                    && cmd.contains("--stream")),
            "expected finalize command to carry --done, got: {:?}",
            plan.required_commands
        );
    }

    #[test]
    fn build_plan_resolves_each_id_in_compound_do_directive() {
        let dir = setup_project();
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
- [ ] [#x63e] First packet target
- [ ] [#v4v0] Second packet target
<!-- /agent:pending -->
"#;

        let current = baseline.replace(
            "<!-- /agent:exchange -->",
            "do [#x63e] [#v4v0]. spec-test-build-install-commit-push\n<!-- /agent:exchange -->",
        );

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        assert_eq!(
            plan.pending_mutations
                .iter()
                .map(|mutation| mutation.id.as_str())
                .collect::<Vec<_>>(),
            vec!["x63e", "v4v0"]
        );
        assert!(
            plan.required_commands
                .iter()
                .any(|cmd| cmd.contains("--done x63e") && cmd.contains("--done v4v0")),
            "expected finalize command to carry both --done flags, got: {:?}",
            plan.required_commands
        );
    }

    #[test]
    fn build_plan_emits_lower_agent_routing_fields() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
agent_doc_dispatch: auto
---

## Exchange

<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->

## Pending

<!-- agent:backlog -->
- [ ] [#jobp1] Define lower-agent job packet spec and runbook.
- [ ] [#jobp2] Add tsift context packet tests.
<!-- /agent:backlog -->
"#;

        let current = baseline.replace(
            "<!-- /agent:exchange -->",
            "do [#jobp1]\ndo [#jobp2]\n<!-- /agent:exchange -->",
        );

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        assert!(plan.dispatch_candidate);
        assert_eq!(plan.dispatch_mode, "auto");
        assert_eq!(plan.task_class, "lower_agent_orchestration");
        assert_eq!(plan.risk, "high");
        assert!(plan.parallelizable);
        assert_eq!(plan.model_tier, "high");
        assert_eq!(plan.context_budget_tokens, 10_000);
        assert!(
            plan.write_scope
                .contains(&"src/agent-doc/specs/".to_string())
        );
        assert!(plan.write_scope.contains(&"src/tsift/".to_string()));
        assert!(plan.required_proof.contains(&"verification".to_string()));
        assert_eq!(plan.tsift_context.status, "missing");
        assert!(
            plan.required_commands
                .iter()
                .any(|cmd| cmd.contains("--done jobp1") && cmd.contains("--done jobp2"))
        );
    }

    #[test]
    fn build_plan_resolves_existing_icebox_item_for_do_directive() {
        let dir = setup_project();
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
        let dir = setup_project();
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
        let dir = setup_project();
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
    fn plan_expect_add_carries_explicit_backlog_target() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let target = dir.path().join("bugs.md");
        std::fs::write(
            &target,
            "<!-- agent:backlog -->\n- [ ] [#old1] Existing\n<!-- /agent:backlog -->\n",
        )
        .unwrap();

        let baseline = format!(
            r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
prompt_presets:
  '#agent-doc-bug': Please create a plan. Add to the backlog of {}
---

## Exchange

<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->
"#,
            target.display()
        );

        let current = baseline.replace(
            "<!-- /agent:exchange -->",
            "#agent-doc-bug\n<!-- /agent:exchange -->",
        );
        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, &baseline).unwrap();

        let plan = build(&doc).unwrap();
        let expect_add = plan
            .pending_mutations
            .iter()
            .find(|m| m.kind == PendingMutationKind::ExpectAdd)
            .expect("expected ExpectAdd mutation");
        assert_eq!(
            expect_add.target_files,
            vec![
                std::fs::canonicalize(&target)
                    .unwrap()
                    .display()
                    .to_string()
            ]
        );
        assert!(
            plan.required_commands
                .iter()
                .any(|cmd| cmd.contains("--pending-add-to") && cmd.contains("bugs.md")),
            "expected finalize hint to include --pending-add-to, got {:?}",
            plan.required_commands
        );
    }

    #[test]
    fn plan_preserves_agent_doc_bug_declaration_order_for_target_adds() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let target = dir.path().join("bugs.md");
        std::fs::write(
            &target,
            "<!-- agent:backlog -->\n- [ ] [#old1] Existing\n<!-- /agent:backlog -->\n",
        )
        .unwrap();

        let baseline = format!(
            r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
prompt_presets:
  '#agent-doc-bug': Please create a plan. Add to the backlog of {}
---

## Exchange

<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->
"#,
            target.display()
        );

        let current = baseline.replace(
            "<!-- /agent:exchange -->",
            "First captured bug. #agent-doc-bug\n---\nSecond captured bug. #agent-doc-bug\n<!-- /agent:exchange -->",
        );
        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, &baseline).unwrap();

        let plan = build(&doc).unwrap();
        let expect_adds = plan
            .pending_mutations
            .iter()
            .filter(|mutation| mutation.kind == PendingMutationKind::ExpectAdd)
            .collect::<Vec<_>>();

        assert_eq!(expect_adds.len(), 2);
        assert_eq!(expect_adds[0].text, "First captured bug. #agent-doc-bug");
        assert_eq!(expect_adds[1].text, "Second captured bug. #agent-doc-bug");
        assert_eq!(expect_adds[0].target_files, expect_adds[1].target_files);
    }

    #[test]
    fn test_plan_detects_recommendation_request() {
        let dir = setup_project();
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
        let dir = setup_project();
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
        let dir = setup_project();
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
        let dir = setup_project();
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
        let dir = setup_project();
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
        let dir = setup_project();
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
        let dir = setup_project();
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

    #[test]
    fn build_plan_marks_agent_doc_bug_prompt_as_plan_backlog_only() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let content = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/bugs.md
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

        let _prompt = EnvGuard::set("AGENT_DOC_HARNESS_PROMPT", "#agent-doc-bug");
        let plan = build(&doc).unwrap();

        assert_eq!(plan.execution_scope, ExecutionScope::PlanBacklogOnly);
        assert!(plan.repo_actions.is_empty(), "{:?}", plan.repo_actions);
        assert!(
            plan.pending_mutations
                .iter()
                .any(|m| m.kind == PendingMutationKind::ExpectAdd),
            "expected ExpectAdd from #agent-doc-bug preset expansion, got {:?}",
            plan.pending_mutations
        );
    }

    #[test]
    fn build_plan_does_not_treat_backlog_text_as_agent_doc_bug_prompt_scope() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/bugs.md
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

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
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/bugs.md
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.

do #pbct. spec-test-build-install-commit-push
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#pbct] Respect `#agent-doc-bug` preset scope and fail closed before implementation.
<!-- /agent:backlog -->
"#;

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        assert_eq!(plan.execution_scope, ExecutionScope::Normal);
        assert_eq!(
            plan.repo_actions,
            vec!["do #pbct. spec-test-build-install-commit-push".to_string()]
        );
    }

    #[test]
    fn build_plan_keeps_copied_prompt_preset_definitions_out_of_prompt_scope() {
        let dir = setup_project();
        let doc = dir.path().join("tmux-router.md");

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

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.

do #tmuxreprocmd. spec-test-build-install-commit-push
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#tmuxreprocmd] Capture the exact command, crate root, and tooling context that produced the tmux-router diagnostic.
<!-- /agent:backlog -->
"#;

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        assert_eq!(plan.execution_scope, ExecutionScope::Normal);
        assert_eq!(
            plan.repo_actions,
            vec!["do #tmuxreprocmd. spec-test-build-install-commit-push".to_string()]
        );
        assert!(
            !plan
                .pending_mutations
                .iter()
                .any(|mutation| mutation.kind == PendingMutationKind::ExpectAdd),
            "copied preset definitions must not require agent-doc-bug backlog capture: {:?}",
            plan.pending_mutations
        );
    }

    #[test]
    fn build_plan_does_not_block_on_session_accretion_guard() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let long_exchange = (0..260)
            .map(|idx| format!("context line {idx}"))
            .collect::<Vec<_>>()
            .join("\n");

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

        let current = format!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n{long_exchange}\ndo #ctxacc. spec-test-build-install-commit-push\n<!-- /agent:exchange -->\n"
        );

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        assert!(plan.blockers.is_empty(), "unexpected blockers: {:?}", plan);
        assert_eq!(
            plan.repo_actions,
            vec!["do #ctxacc. spec-test-build-install-commit-push".to_string()]
        );
    }

    #[test]
    fn build_plan_keeps_repeated_noop_closeout_churn_advisory() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

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

do #nooploop. spec-test-build-install-commit-push
<!-- /agent:exchange -->
"#;

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();
        write_cycles_log(
            &doc,
            &[
                crate::ops_log::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(20).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                crate::ops_log::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(10).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
            ],
        );

        let plan = build(&doc).unwrap();

        assert_eq!(plan.handoff, HandoffTarget::None);
        assert_eq!(
            plan.repo_actions,
            vec!["do #nooploop. spec-test-build-install-commit-push".to_string()],
            "session-accretion no-op churn should remain advisory unless compact is explicit"
        );
        assert!(
            !plan
                .required_commands
                .iter()
                .any(|command| command.contains("agent-doc compact")),
            "session-accretion no-op churn must not force compaction: {:?}",
            plan.required_commands
        );
        assert!(
            plan.required_commands
                .iter()
                .any(|command| command.contains("agent-doc finalize")),
            "normal closeout should still be requested after repo work: {:?}",
            plan.required_commands
        );
    }

    #[test]
    fn build_plan_allows_turn_after_recent_compaction_recovery() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

Compacted.
<!-- /agent:exchange -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

Compacted.
do #cmpclr. spec-test-build-install-commit-push
<!-- /agent:exchange -->
"#;

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();
        write_cycles_log(
            &doc,
            &[
                crate::ops_log::CycleEntry {
                    op: "commit".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(120).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                crate::ops_log::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(110).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                crate::ops_log::CycleEntry {
                    op: "commit".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(100).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                crate::ops_log::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(90).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                crate::ops_log::CycleEntry {
                    op: "commit".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(80).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                crate::ops_log::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(70).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                crate::ops_log::CycleEntry {
                    op: "commit".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(60).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                crate::ops_log::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(50).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                crate::ops_log::CycleEntry {
                    op: "commit".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(40).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                crate::ops_log::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(30).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
            ],
        );
        crate::session_accretion::record_recent_exchange_compaction(&doc).unwrap();

        let plan = build(&doc).unwrap();

        assert!(
            plan.blockers.is_empty(),
            "recent exchange compaction should clear closeout-churn blockers: {:?}",
            plan.blockers
        );
        assert_eq!(
            plan.repo_actions,
            vec!["do #cmpclr. spec-test-build-install-commit-push".to_string()]
        );
    }

    #[test]
    fn build_plan_allows_post_compaction_rerun_noop_closeouts() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

Compacted.
<!-- /agent:exchange -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

Compacted.
do #aftercmp. spec-test-build-install-commit-push
<!-- /agent:exchange -->
"#;

        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();
        crate::session_accretion::record_recent_exchange_compaction(&doc).unwrap();
        write_cycles_log(
            &doc,
            &[
                crate::ops_log::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(5).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                crate::ops_log::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(4).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
            ],
        );

        let plan = build(&doc).unwrap();

        assert_eq!(
            plan.handoff,
            HandoffTarget::None,
            "preflight no-op closeouts immediately after compact must not trap the rerun in another compact handoff"
        );
        assert_eq!(
            plan.repo_actions,
            vec!["do #aftercmp. spec-test-build-install-commit-push".to_string()]
        );
    }
}
