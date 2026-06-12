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
//! - Downgrades tsift graph evidence collection failures to `manual_packet_only`
//!   warnings so agent-doc turns and job packet creation can continue without
//!   graph acceptance evidence.
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
    frontmatter,
};
use agent_doc_orchestration::{diff, pending, security};

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
    pub manual_packet_only: bool,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
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
        agent_doc_orchestration::harness_prompt::synthetic_diff_for_file(file)?
    } else {
        None
    };
    let queue_prompt = if doc_diff.is_none() && harness_diff.is_none() {
        active_queue_prompt(&content)
    } else {
        None
    };
    let queue_head_slash_command = queue_prompt
        .as_deref()
        .and_then(agent_doc_orchestration::queue_command::slash_command_text);
    let queue_diff = queue_prompt
        .as_deref()
        .map(|prompt| diff::synthetic_added_lines_diff(prompt, "queue"));

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
            manual_packet_only: false,
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
            warnings: Vec::new(),
        });
    };

    let queue_active_for_prompt_extraction = queue_is_active_for_diff(&content, &diff_text);
    let command_diff_text = if queue_active_for_prompt_extraction {
        diff_text.clone()
    } else {
        diff::suppress_inactive_queue_additions(&diff_text, &content)
    };
    let prompt_diff_text = if queue_head_slash_command.is_some() {
        String::new()
    } else {
        command_diff_text.clone()
    };

    let prompt_bearing_changes = diff::classify_prompt_bearing_changes(&prompt_diff_text);
    let prompt_targets = agent_doc_orchestration::flow::session_cycle::prompt_targets_from_changes(
        &prompt_bearing_changes,
    );
    let added_diff_lines =
        agent_doc_orchestration::prompt_contract::collect_added_diff_lines(&prompt_diff_text);

    let execution_scope = execution_scope_for_prompt_targets(
        &prompt_targets,
        &added_diff_lines,
        harness_diff.is_some(),
        &fm.prompt_presets,
    );
    let repo_actions = if execution_scope == ExecutionScope::PlanBacklogOnly {
        Vec::new()
    } else {
        diff::extract_imperative_directives(&prompt_diff_text)
    };
    let orchestration_request = diff::detect_orchestration_request(&prompt_diff_text);
    let exchange_compaction_requested = diff::detect_exchange_compaction_request(&prompt_diff_text);
    let parsed_commands = diff::parse_slash_commands_classified(&command_diff_text);
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
    let mut warnings = Vec::new();
    if let Some(deferral) = plan_backlog_only_deferral_warning(execution_scope, &prompt_diff_text) {
        warnings.push(deferral);
    }
    match agent_doc_orchestration::memory_cmd::semantic_completion_matches(file, None, 5) {
        Ok(matches) => {
            warnings.extend(
                matches
                    .iter()
                    .map(agent_doc_orchestration::memory_cmd::format_semantic_completion_warning),
            );
        }
        Err(err) => warnings.push(format!("semantic completion retrieval unavailable: {err}")),
    }
    let mut manual_packet_only = false;
    let graph_targets = prompt_targets
        .iter()
        .chain(repo_actions.iter())
        .cloned()
        .collect::<Vec<_>>();
    let graph_evidence = match crate::tsift_graph::collect_for_do_items(file, &graph_targets) {
        Ok(graph_evidence) => graph_evidence,
        Err(err) => {
            manual_packet_only = true;
            warnings.push(crate::tsift_graph::manual_packet_fallback_warning(&err));
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
        manual_packet_only,
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
        warnings,
    })
}

fn active_queue_prompt(content: &str) -> Option<String> {
    let components = component::parse(content).ok()?;
    let queue_component = components
        .iter()
        .find(|component| component.name == "queue")?;
    let body = &content[queue_component.open_end..queue_component.close_start];
    let entries = agent_doc_orchestration::queue::parse(body).ok()?;
    let has_auto = agent_doc_orchestration::queue::has_auto_attr(&queue_component.attrs);
    let (fm, _) = frontmatter::parse(content).ok()?;
    let activation = agent_doc_orchestration::queue::resolve_activation(
        &entries,
        has_auto,
        false,
        fm.queue_active.unwrap_or(false),
    );
    if !activation.active {
        return None;
    }
    agent_doc_orchestration::queue::prompts(&activation.entries_after)
        .first()
        .map(|prompt| prompt.text.clone())
}

fn queue_is_active_for_diff(content: &str, diff_text: &str) -> bool {
    let Ok(components) = component::parse(content) else {
        return false;
    };
    let Some(queue_component) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return false;
    };
    let body = &content[queue_component.open_end..queue_component.close_start];
    let Ok(entries) = agent_doc_orchestration::queue::parse(body) else {
        return false;
    };
    let has_auto = agent_doc_orchestration::queue::has_auto_attr(&queue_component.attrs);
    let (fm, _) = frontmatter::parse(content).unwrap_or_default();
    agent_doc_orchestration::queue::resolve_activation(
        &entries,
        has_auto,
        diff::detect_queue_trigger(diff_text),
        fm.queue_active.unwrap_or(false),
    )
    .active
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
    match agent_doc_orchestration::flow::session_cycle::classify_execution_scope(
        prompt_targets,
        added_diff_lines,
        harness_prompt_only,
        prompt_presets,
    ) {
        agent_doc_orchestration::flow::session_cycle::SessionExecutionScope::PlanBacklogOnly => {
            ExecutionScope::PlanBacklogOnly
        }
        agent_doc_orchestration::flow::session_cycle::SessionExecutionScope::Normal => {
            ExecutionScope::Normal
        }
    }
}

/// `#lr-queue-response-miss` (step 2): when a `#agent-doc-bug` capture forces
/// `execution_scope=plan_backlog_only` but the same prompt diff also carries
/// runnable imperative directives (a `do [#id]` head, `build + push`, etc.),
/// surface the deferral explicitly so the actionable work is not silently
/// suppressed. Returns `None` for the pure bug-capture case (no imperative
/// directive) so normal plan-only turns stay quiet.
fn plan_backlog_only_deferral_warning(
    execution_scope: ExecutionScope,
    prompt_diff_text: &str,
) -> Option<String> {
    if execution_scope != ExecutionScope::PlanBacklogOnly {
        return None;
    }
    let deferred = diff::extract_imperative_directives(prompt_diff_text);
    if deferred.is_empty() {
        return None;
    }
    Some(format!(
        "execution_scope=plan_backlog_only suppressed runnable directive(s) this cycle to capture the #agent-doc-bug plan/backlog: [{}]. The deferral is explicit — run the directive(s) in a later cycle.",
        deferred.join("; ")
    ))
}

fn finalize_placeholder_commands(
    file: &Path,
    fm: &frontmatter::Frontmatter,
    pending_mutations: &[PendingMutationPlan],
) -> Vec<String> {
    let pending = pending_mutations
        .iter()
        .map(
            |mutation| agent_doc_orchestration::flow::session_cycle::FinalizePendingMutation {
                kind: match mutation.kind {
                    PendingMutationKind::ResolveExisting => {
                        agent_doc_orchestration::flow::session_cycle::FinalizePendingMutationKind::ResolveExisting
                    }
                    PendingMutationKind::ExpectAdd => {
                        agent_doc_orchestration::flow::session_cycle::FinalizePendingMutationKind::ExpectAdd
                    }
                },
                id: &mutation.id,
                target_files: &mutation.target_files,
            },
        )
        .collect::<Vec<_>>();
    vec![
        agent_doc_orchestration::flow::session_cycle::finalize_command(
            file,
            fm.resolve_mode(),
            &pending,
        ),
    ]
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
    let root = agent_doc_orchestration::snapshot::find_project_root(&canonical)
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
        stale_fallback: "skip tsift graph/context evidence, continue with parent-owned execution or manual packets, and carry the diagnostic for review".to_string(),
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
            push_resolve_existing_mutation(&mut pending_mutations, &items, &id);
        }
    }

    let auto_done =
        agent_doc_orchestration::session_check::resolve_auto_done(file).unwrap_or(false);
    for id in agent_doc_orchestration::session_check::inline_done_signal_ids(
        file,
        prompt_targets,
        auto_done,
    )? {
        push_resolve_existing_mutation(&mut pending_mutations, &items, &id);
    }

    if agent_doc_orchestration::prompt_contract::prompt_requests_backlog_work(
        prompt_targets,
        added_diff_lines,
        &fm.prompt_presets,
    ) {
        let target_files = agent_doc_orchestration::prompt_contract::explicit_backlog_targets(
            file,
            prompt_targets,
            added_diff_lines,
            &fm.prompt_presets,
        )?
        .into_iter()
        .map(|path| path.display().to_string())
        .collect();
        let issue_units =
            agent_doc_orchestration::prompt_contract::ordered_issue_units_for_agent_doc_bug(
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

fn push_resolve_existing_mutation(
    pending_mutations: &mut Vec<PendingMutationPlan>,
    items: &[pending::PendingItem],
    id: &str,
) {
    let Some(item) = items
        .iter()
        .find(|item| item.id.eq_ignore_ascii_case(id) && item.state != pending::PendingState::Done)
    else {
        return;
    };
    if pending_mutations
        .iter()
        .any(|mutation| mutation.id == item.id)
    {
        return;
    }
    pending_mutations.push(PendingMutationPlan {
        kind: PendingMutationKind::ResolveExisting,
        id: item.id.clone(),
        text: item.text.clone(),
        target_files: Vec::new(),
    });
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
mod tests;
