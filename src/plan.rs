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

use agent_doc_hash::content_hash;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use agent_doc_element::element::{self, is_backlog_component, is_tracked_work_component};
use agent_doc_element_backlog::backlog;

use agent_doc_diff as diff;
use agent_doc_frontmatter::frontmatter;
use agent_doc_orchestration::{diff_io, frontmatter_io};
use agent_doc_workflow::session_cycle::{
    FinalizePendingMutation, FinalizePendingMutationKind, SessionExecutionScope,
    classify_execution_scope, finalize_command, prompt_targets_from_changes,
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
    pub loaded_context_ledger: LoadedContextLedger,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LoadedContextLedger {
    pub entries: Vec<LoadedContextRecord>,
    pub duplicate_expansions_suppressed: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LoadedContextRecord {
    pub source_id: String,
    pub source_kind: String,
    pub path: String,
    pub content_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concept_id: Option<String>,
    pub loaded_at: String,
    pub expansion_reason: String,
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
    let (fm, _body) = frontmatter_io::parse_for_file(&content, file)
        .with_context(|| format!("failed to parse frontmatter in {}", file.display()))?;

    let doc_diff = diff_io::compute(file)?;
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
        .and_then(agent_doc_queue::queue_command::slash_command_text);
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
    let prompt_targets = prompt_targets_from_changes(&prompt_bearing_changes);
    let added_diff_lines = agent_doc_prompt_contract::collect_added_diff_lines(&prompt_diff_text);

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
    let components = element::parse(content).ok()?;
    let queue_component = components
        .iter()
        .find(|component| component.name == "queue")?;
    let body = &content[queue_component.open_end..queue_component.close_start];
    let entries = agent_doc_queue::document_queue::parse(body).ok()?;
    let has_auto = agent_doc_queue::document_queue::has_auto_attr(&queue_component.attrs);
    let (fm, _) = frontmatter::parse(content).ok()?;
    let activation = agent_doc_queue::document_queue::resolve_activation(
        &entries,
        has_auto,
        false,
        fm.queue_active.unwrap_or(false),
    );
    if !activation.active {
        return None;
    }
    agent_doc_queue::document_queue::prompts(&activation.entries_after)
        .first()
        .map(|prompt| agent_doc_queue::document_queue::strip_in_progress_marker(&prompt.text))
}

fn queue_is_active_for_diff(content: &str, diff_text: &str) -> bool {
    let Ok(components) = element::parse(content) else {
        return false;
    };
    let Some(queue_component) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return false;
    };
    let body = &content[queue_component.open_end..queue_component.close_start];
    let Ok(entries) = agent_doc_queue::document_queue::parse(body) else {
        return false;
    };
    let has_auto = agent_doc_queue::document_queue::has_auto_attr(&queue_component.attrs);
    let (fm, _) = frontmatter::parse(content).unwrap_or_default();
    agent_doc_queue::document_queue::resolve_activation(
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
    match classify_execution_scope(
        prompt_targets,
        added_diff_lines,
        harness_prompt_only,
        prompt_presets,
    ) {
        SessionExecutionScope::PlanBacklogOnly => ExecutionScope::PlanBacklogOnly,
        SessionExecutionScope::Normal => ExecutionScope::Normal,
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
        .map(|mutation| FinalizePendingMutation {
            kind: match mutation.kind {
                PendingMutationKind::ResolveExisting => {
                    FinalizePendingMutationKind::ResolveExisting
                }
                PendingMutationKind::ExpectAdd => FinalizePendingMutationKind::ExpectAdd,
            },
            id: &mutation.id,
            target_files: &mutation.target_files,
        })
        .collect::<Vec<_>>();
    vec![finalize_command(file, fm.resolve_mode(), &pending)]
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
    let root = agent_doc_fs::find_project_root(&canonical)
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
    let index_status_command = format!("tsift status --json {root_arg}");
    let context_pack_command = format!("tsift context-pack {file_arg} --json --budget normal");
    let diff_digest_command = format!("tsift diff-digest --cached {root_arg} --json");
    let test_digest_command =
        format!("tsift test-digest --path {root_arg} --input <test.log> --json");
    let loaded_context_ledger = build_loaded_context_ledger(vec![
        loaded_context_record(
            "agent-doc.plan.tsift-status",
            "generated-state",
            &display_path(&root),
            &index_status_command,
            None,
            "plan_build",
            "verify tsift index availability for this cycle",
        ),
        loaded_context_record(
            "agent-doc.plan.context-pack",
            "generated-state",
            &display_path(file),
            &context_pack_command,
            None,
            "plan_build",
            "declare bounded context-pack source for this cycle",
        ),
        loaded_context_record(
            "agent-doc.plan.diff-digest",
            "generated-state",
            &display_path(&root),
            &diff_digest_command,
            None,
            "plan_build",
            "declare bounded diff evidence source for this cycle",
        ),
        loaded_context_record(
            "agent-doc.plan.test-digest",
            "generated-state",
            &display_path(&root),
            &test_digest_command,
            None,
            "plan_build",
            "declare bounded test-output source for this cycle",
        ),
    ]);
    TsiftContextPlan {
        status: status.to_string(),
        freshness_policy: "fresh context required for automatic dispatch; manual packets record stale or missing context explicitly".to_string(),
        index_status_command,
        context_pack_command,
        source_read_command: "tsift source-read <handle>".to_string(),
        diff_digest_command,
        test_digest_command,
        stale_fallback: "skip tsift graph/context evidence, continue with parent-owned execution or manual packets, and carry the diagnostic for review".to_string(),
        loaded_context_ledger,
    }
}

pub(crate) fn loaded_context_record(
    source_id: &str,
    source_kind: &str,
    path: &str,
    content: &str,
    concept_id: Option<&str>,
    loaded_at: &str,
    expansion_reason: &str,
) -> LoadedContextRecord {
    LoadedContextRecord {
        source_id: source_id.to_string(),
        source_kind: source_kind.to_string(),
        path: path.to_string(),
        content_hash: content_hash(content),
        concept_id: concept_id.map(str::to_string),
        loaded_at: loaded_at.to_string(),
        expansion_reason: expansion_reason.to_string(),
    }
}

pub(crate) fn build_loaded_context_ledger(
    records: Vec<LoadedContextRecord>,
) -> LoadedContextLedger {
    let mut entries_by_key: HashMap<(String, String), LoadedContextRecord> = HashMap::new();
    let mut duplicate_expansions_suppressed = 0;
    for record in records {
        let key = (record.source_id.clone(), record.content_hash.clone());
        if let std::collections::hash_map::Entry::Vacant(entry) = entries_by_key.entry(key) {
            entry.insert(record);
        } else {
            duplicate_expansions_suppressed += 1;
        }
    }
    let mut entries = entries_by_key.into_values().collect::<Vec<_>>();
    entries.sort_by(|a, b| {
        a.source_id
            .cmp(&b.source_id)
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.content_hash.cmp(&b.content_hash))
    });
    LoadedContextLedger {
        entries,
        duplicate_expansions_suppressed,
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
    let components = element::parse(content).context("failed to parse document components")?;
    let has_backlog = components
        .iter()
        .any(|component| is_backlog_component(&component.name));

    if !has_backlog {
        return Ok(Vec::new());
    }

    let items: Vec<backlog::PendingItem> = components
        .iter()
        .filter(|component| is_tracked_work_component(&component.name))
        .flat_map(|component| {
            let (_, items, _) = backlog::parse_items(component.content(content));
            items
        })
        .collect();
    let mut pending_mutations: Vec<PendingMutationPlan> = Vec::new();

    for action in repo_actions {
        for id in extract_do_pending_ids(action) {
            push_resolve_existing_mutation(&mut pending_mutations, &items, &id);
        }
    }

    let project_config = agent_doc_project_config_io::load_project_for_doc(file);
    let auto_done = agent_doc_frontmatter::project_config::resolve_auto_done(&fm, &project_config);
    for id in agent_doc_orchestration::session_check::inline_done_signal_ids(
        file,
        prompt_targets,
        auto_done,
    )? {
        push_resolve_existing_mutation(&mut pending_mutations, &items, &id);
    }

    if agent_doc_prompt_contract::prompt_requests_backlog_work(
        prompt_targets,
        added_diff_lines,
        &fm.prompt_presets,
    ) {
        let issue_units = agent_doc_prompt_contract::ordered_issue_units_for_agent_doc_bug(
            prompt_targets,
            added_diff_lines,
            &fm.prompt_presets,
            prompt_bearing_changes,
        );
        let mut target_paths = agent_doc_prompt_contract::explicit_backlog_targets(
            file,
            prompt_targets,
            added_diff_lines,
            &fm.prompt_presets,
        )?;
        if target_paths.is_empty()
            && !issue_units.is_empty()
            && let Some(target) =
                agent_doc_project_config_io::agent_doc_bug_target_document_for_doc(file)?
        {
            target_paths.push(target);
        }
        let target_files = target_paths
            .into_iter()
            .map(|path| path.display().to_string())
            .collect();
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
    items: &[backlog::PendingItem],
    id: &str,
) {
    let Some(item) = items
        .iter()
        .find(|item| item.id.eq_ignore_ascii_case(id) && item.state != backlog::PendingState::Done)
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
    agent_doc_queue::queue_directive::explicit_do_directive_target_ids(action)
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
            let referenced = agent_doc_fs::referenced_markdown_path(file, &mutation.text)?;
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
    use agent_doc_orchestration::snapshot;
    use std::io::Write;
    use tempfile::TempDir;

    struct EnvGuard {
        key: &'static str,
        prev: Option<String>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let lock = crate::test_support::TEST_ENV_LOCK.lock().unwrap();
            let prev = std::env::var(key).ok();
            unsafe { std::env::set_var(key, value) };
            Self {
                key,
                prev,
                _lock: lock,
            }
        }

        fn unset(key: &'static str) -> Self {
            let lock = crate::test_support::TEST_ENV_LOCK.lock().unwrap();
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

    fn write_cycles_log(
        doc: &std::path::Path,
        entries: &[agent_doc_orchestration::ops_log::CycleEntry],
    ) {
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
    fn build_plan_treats_active_queue_slash_command_as_command_handoff() {
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
-   /clear
<!-- /agent:queue -->
"#;
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let plan = build(&doc).unwrap();

        assert!(plan.prompt_targets.is_empty(), "{plan:?}");
        assert!(plan.repo_actions.is_empty(), "{plan:?}");
        assert!(plan.pending_mutations.is_empty(), "{plan:?}");
        assert_eq!(plan.handoff, HandoffTarget::Other);
        assert!(
            plan.required_commands
                .iter()
                .any(|cmd| cmd.contains("`/clear`")),
            "slash command handoff should remain visible as a command requirement: {:?}",
            plan.required_commands
        );
        assert!(
            plan.required_commands
                .iter()
                .all(|cmd| !cmd.contains("agent-doc finalize")),
            "slash-only command handoff must not require assistant finalization: {:?}",
            plan.required_commands
        );
    }

    #[test]
    fn build_plan_warns_on_semantic_completion_match_for_free_text_queue() {
        let _prompt = EnvGuard::unset("AGENT_DOC_HARNESS_PROMPT");
        let dir = setup_project();
        std::fs::write(
            dir.path().join("tasks.done.md"),
            "- 2026-06-07 [#cachefix] Repair cache duplication\n",
        )
        .unwrap();
        let doc = dir.path().join("plan.md");
        let content = r#"---
agent_doc_session: test
queue_active: true
---

<!-- agent:queue auto -->
- Repair cache duplication
<!-- /agent:queue -->

<!-- agent:done archive=tasks.done.md -->
<!-- /agent:done -->
"#;
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let plan = build(&doc).unwrap();
        assert_eq!(plan.prompt_targets, vec!["Repair cache duplication"]);
        assert!(
            plan.warnings.iter().any(|warning| {
                warning.contains("semantic completion candidate") && warning.contains("#cachefix")
            }),
            "{:?}",
            plan.warnings
        );
    }

    #[test]
    fn build_plan_ignores_inactive_queue_edit_as_repo_action() {
        let _prompt = EnvGuard::unset("AGENT_DOC_HARNESS_PROMPT");
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
queue_active: false
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->

<!-- agent:queue -->
<!-- /agent:queue -->

<!-- agent:backlog -->
- [ ] [#gdbpropscan] Inspect graph DB properties.
<!-- /agent:backlog -->
"#;
        let current = baseline.replace(
            "<!-- agent:queue -->\n<!-- /agent:queue -->",
            "<!-- agent:queue -->\n- do [#gdbpropscan]\n<!-- /agent:queue -->",
        );
        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        assert!(
            plan.prompt_targets.is_empty(),
            "inactive queue edit must not become prompt targets: {:?}",
            plan.prompt_targets
        );
        assert!(
            plan.repo_actions.is_empty(),
            "inactive queue edit must not become repo actions: {:?}",
            plan.repo_actions
        );
        assert!(
            plan.pending_mutations.is_empty(),
            "inactive queue edit must not resolve or capture pending work: {:?}",
            plan.pending_mutations
        );
    }

    #[cfg(unix)]
    #[test]
    fn build_plan_downgrades_locked_graph_db_to_manual_packet_only_warning() {
        use std::os::unix::fs::PermissionsExt;

        let dir = setup_project();
        std::fs::create_dir_all(dir.path().join(".tsift")).unwrap();
        std::fs::write(dir.path().join(".tsift/graph.db"), "fake").unwrap();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#jobslock] Create job packets when graph.db is locked.
<!-- /agent:backlog -->
"#;

        let current = baseline.replace(
            "<!-- /agent:exchange -->",
            "do [#jobslock]. spec-test-build-install-commit-push\n<!-- /agent:exchange -->",
        );
        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let script = dir.path().join("fake-tsift-lock.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\nif echo \"$*\" | grep -q 'graph-db.*--json status'; then echo 'Error code 5: The database file is locked' >&2; exit 1; fi\necho \"unexpected fake tsift args: $*\" >&2\nexit 2\n",
        )
        .unwrap();
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();
        let _env = EnvGuard::set("AGENT_DOC_TSIFT_BIN", script.to_str().unwrap());

        let plan = build(&doc).unwrap();

        assert!(plan.blockers.is_empty(), "unexpected blockers: {:?}", plan);
        assert!(plan.manual_packet_only);
        assert!(plan.graph_evidence.is_none());
        assert!(
            plan.warnings
                .iter()
                .any(|warning| warning.contains("database file is locked")),
            "expected lock warning, got {:?}",
            plan.warnings
        );
        assert_eq!(
            plan.repo_actions,
            vec!["do [#jobslock]. spec-test-build-install-commit-push"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn build_plan_downgrades_stale_graph_db_to_manual_packet_only_warning() {
        use std::os::unix::fs::PermissionsExt;

        let dir = setup_project();
        std::fs::create_dir_all(dir.path().join(".tsift")).unwrap();
        std::fs::write(dir.path().join(".tsift/graph.db"), "fake").unwrap();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#staleg] Keep turns running when graph.db is stale.
<!-- /agent:backlog -->
"#;

        let current = baseline.replace(
            "<!-- /agent:exchange -->",
            "do [#staleg]. spec-test-build-install-commit-push\n<!-- /agent:exchange -->",
        );
        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let script = dir.path().join("fake-tsift-stale.sh");
        std::fs::write(
            &script,
            r#"#!/bin/sh
if echo "$*" | grep -q 'graph-db.*--json status'; then
  cat <<'JSON'
{"root":"/tmp/repo","graph_db":"/tmp/repo/.tsift/graph.db","freshness":{"status":"stale","fail_closed":true,"diagnostics":["graph.db is stale"]}}
JSON
  exit 0
fi
echo "unexpected fake tsift args: $*" >&2
exit 2
"#,
        )
        .unwrap();
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();
        let _env = EnvGuard::set("AGENT_DOC_TSIFT_BIN", script.to_str().unwrap());

        let plan = build(&doc).unwrap();

        assert!(plan.blockers.is_empty(), "unexpected blockers: {:?}", plan);
        assert!(plan.manual_packet_only);
        assert!(plan.graph_evidence.is_none());
        assert!(
            plan.warnings
                .iter()
                .any(|warning| warning.contains("graph.db is stale")
                    && warning.contains("manual_packet_only=true")),
            "expected stale graph warning, got {:?}",
            plan.warnings
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
    fn loaded_context_ledger_suppresses_duplicate_source_hashes() {
        let first = loaded_context_record(
            "agent-doc.test.source",
            "procedure",
            "runbooks/respond.md",
            "same content",
            None,
            "test",
            "first expansion",
        );
        let duplicate = loaded_context_record(
            "agent-doc.test.source",
            "procedure",
            "runbooks/respond.md",
            "same content",
            None,
            "test",
            "duplicate expansion",
        );
        let distinct = loaded_context_record(
            "agent-doc.test.source",
            "procedure",
            "runbooks/respond.md",
            "changed content",
            None,
            "test",
            "changed source expansion",
        );

        let ledger = build_loaded_context_ledger(vec![first, duplicate, distinct]);

        assert_eq!(ledger.entries.len(), 2);
        assert_eq!(ledger.duplicate_expansions_suppressed, 1);
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
    fn agent_doc_bug_uses_configured_target_when_preset_has_no_explicit_file() {
        let dir = setup_project();
        std::fs::write(
            dir.path().join(".agent-doc/config.toml"),
            r#"agent_doc_bug_target_document = "tasks/agent-doc/agent-doc-bugs2.md"
"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("tasks/agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join("tasks/software")).unwrap();
        let doc = dir.path().join("tasks/software/source.md");
        let target = dir.path().join("tasks/agent-doc/agent-doc-bugs2.md");
        std::fs::write(
            &target,
            "<!-- agent:backlog -->\n- [ ] [#old1] Existing\n<!-- /agent:backlog -->\n",
        )
        .unwrap();

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog.
---

## Exchange

<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->
"#;

        let current = baseline.replace(
            "<!-- /agent:exchange -->",
            "#agent-doc-bug\n<!-- /agent:exchange -->",
        );
        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();
        let expect_add = plan
            .pending_mutations
            .iter()
            .find(|m| m.kind == PendingMutationKind::ExpectAdd)
            .expect("expected ExpectAdd mutation");
        assert_eq!(
            expect_add.target_files,
            vec![target.canonicalize().unwrap().display().to_string()]
        );
    }

    #[test]
    fn agent_doc_bug_explicit_target_overrides_configured_target() {
        let dir = setup_project();
        std::fs::write(
            dir.path().join(".agent-doc/config.toml"),
            r#"agent_doc_bug_target_document = "configured-bugs.md"
"#,
        )
        .unwrap();
        let doc = dir.path().join("plan.md");
        let configured = dir.path().join("configured-bugs.md");
        let explicit = dir.path().join("explicit-bugs.md");
        std::fs::write(
            &configured,
            "<!-- agent:backlog -->\n<!-- /agent:backlog -->\n",
        )
        .unwrap();
        std::fs::write(
            &explicit,
            "<!-- agent:backlog -->\n<!-- /agent:backlog -->\n",
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
            explicit.display()
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
            vec![explicit.canonicalize().unwrap().display().to_string()]
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
    fn plan_classifies_embedded_next_steps_domain_prompt_as_actionable_backlog_request() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
prompt_presets:
  '#next-steps': Any follow-up items to place in the backlog?
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

        let prompt =
            "Please analyze failed orders and bot traffic on sampleorders.com. #next-steps";
        let current = baseline.replace(
            "<!-- /agent:exchange -->",
            &format!("{prompt}\n<!-- /agent:exchange -->"),
        );
        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        assert_eq!(plan.task_class, "prompt_response");
        assert_eq!(plan.execution_scope, ExecutionScope::Normal);
        assert!(
            plan.prompt_targets
                .iter()
                .any(|target| target.contains(prompt)),
            "expected embedded #next-steps domain prompt target, got {:?}",
            plan.prompt_targets
        );
        assert!(
            plan.pending_mutations
                .iter()
                .any(|mutation| mutation.kind == PendingMutationKind::ExpectAdd),
            "expected ExpectAdd from embedded #next-steps preset expansion, got {:?}",
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
    fn build_plan_resolves_explicit_inline_done_signal() {
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
- [ ] [#inline-done-signal] Inline done signal
<!-- /agent:backlog -->
"#;

        let current = baseline.replace(
            "Done.\n<!-- /agent:exchange -->",
            "Done.\n\nmark #inline-done-signal done\n<!-- /agent:exchange -->",
        );

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        assert!(
            plan.pending_mutations.iter().any(|m| {
                m.kind == PendingMutationKind::ResolveExisting && m.id == "inline-done-signal"
            }),
            "expected ResolveExisting for explicit inline done signal, got {:?}",
            plan.pending_mutations
        );
    }

    #[test]
    fn build_plan_resolves_plain_done_to_single_review_item_when_auto_done_enabled() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
auto_done: true
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Waiting.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#rev1] Await user acceptance
<!-- /agent:review -->
"#;

        let current = baseline.replace(
            "Waiting.\n<!-- /agent:exchange -->",
            "Waiting.\n\ndone\n<!-- /agent:exchange -->",
        );

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        assert!(
            plan.pending_mutations
                .iter()
                .any(|m| m.kind == PendingMutationKind::ResolveExisting && m.id == "rev1"),
            "expected ResolveExisting for the single review item, got {:?}",
            plan.pending_mutations
        );
    }

    #[test]
    fn build_plan_does_not_resolve_plain_done_without_auto_done() {
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

Waiting.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#rev1] Await user acceptance
<!-- /agent:review -->
"#;

        let current = baseline.replace(
            "Waiting.\n<!-- /agent:exchange -->",
            "Waiting.\n\ndone\n<!-- /agent:exchange -->",
        );

        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let plan = build(&doc).unwrap();

        assert!(
            !plan
                .pending_mutations
                .iter()
                .any(|m| m.kind == PendingMutationKind::ResolveExisting && m.id == "rev1"),
            "plain done should require auto_done, got {:?}",
            plan.pending_mutations
        );
    }

    #[test]
    fn plan_backlog_only_deferral_warning_names_suppressed_directive() {
        // #lr-queue-response-miss step 2: a runnable directive deferred by
        // plan_backlog_only must be surfaced by name.
        let diff = "--- snapshot\n+++ document\n@@ -0,0 +1 @@\n+do [#lr-queue-response-miss]\n";
        let warning =
            plan_backlog_only_deferral_warning(ExecutionScope::PlanBacklogOnly, diff).unwrap();
        assert!(warning.contains("plan_backlog_only"));
        assert!(
            warning.contains("lr-queue-response-miss"),
            "deferral warning must name the suppressed directive: {warning}"
        );
    }

    #[test]
    fn plan_backlog_only_deferral_warning_quiet_without_directive_or_in_normal_scope() {
        // Pure bug-capture (no imperative directive) stays quiet.
        assert!(
            plan_backlog_only_deferral_warning(
                ExecutionScope::PlanBacklogOnly,
                "+ just a clarifying question about the design?\n"
            )
            .is_none()
        );
        // Normal scope never warns even with a directive.
        assert!(plan_backlog_only_deferral_warning(ExecutionScope::Normal, "+do [#x]\n").is_none());
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
                agent_doc_orchestration::ops_log::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(20).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_orchestration::ops_log::CycleEntry {
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
                agent_doc_orchestration::ops_log::CycleEntry {
                    op: "commit".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(120).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_orchestration::ops_log::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(110).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_orchestration::ops_log::CycleEntry {
                    op: "commit".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(100).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_orchestration::ops_log::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(90).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_orchestration::ops_log::CycleEntry {
                    op: "commit".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(80).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_orchestration::ops_log::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(70).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_orchestration::ops_log::CycleEntry {
                    op: "commit".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(60).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_orchestration::ops_log::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(50).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_orchestration::ops_log::CycleEntry {
                    op: "commit".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(40).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_orchestration::ops_log::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(30).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
            ],
        );
        agent_doc_orchestration::session_accretion::record_recent_exchange_compaction(&doc)
            .unwrap();

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
        agent_doc_orchestration::session_accretion::record_recent_exchange_compaction(&doc)
            .unwrap();
        write_cycles_log(
            &doc,
            &[
                agent_doc_orchestration::ops_log::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(5).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_orchestration::ops_log::CycleEntry {
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
