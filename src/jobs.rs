//! # Module: jobs
//!
//! Binary-owned lower-agent job packet generation.
//!
//! - `agent-doc jobs create <FILE>` turns the current `agent-doc plan` record
//!   into markdown job packets under `.agent-doc/jobs/<cycle>/`.
//! - `agent-doc jobs list/status/collect <FILE>` inspects those local packets
//!   and optional worker result sidecars without mutating the session document.
//! - tsift context is optional in the MVP: packet creation records status,
//!   replay commands, and a context sidecar when `tsift context-pack --json`
//!   succeeds; missing/stale context stays visible and requires parent review.
//! - When the plan downgrades tsift graph evidence failures to
//!   `manual_packet_only`, packet creation continues and the warning is carried
//!   into the job index and packet body.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::plan;
use agent_doc_orchestration::snapshot;

const JOB_PACKET_CONTRACT_VERSION: &str = "agent-doc-job-packet-v1";
const WORKER_RESULT_CONTRACT_VERSION: &str = "agent-doc-worker-result-v1";
const JOB_INDEX_CONTRACT_VERSION: &str = "agent-doc-jobs-index-v1";

fn skip_zero(value: &usize) -> bool {
    *value == 0
}

#[derive(Debug, Clone)]
pub(crate) struct CreateOptions {
    pub(crate) operation_doc: bool,
    pub(crate) audit: bool,
    pub(crate) budget: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct JobsIndex {
    pub(crate) contract_version: String,
    pub(crate) parent_doc: String,
    pub(crate) parent_doc_canonical: String,
    pub(crate) cycle_id: String,
    pub(crate) created_at_unix: u64,
    pub(crate) source_snapshot: String,
    pub(crate) preserve_on_success: bool,
    #[serde(default)]
    pub(crate) manual_packet_only: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) operation_doc: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) auto_dag_schedule_id: Option<String>,
    pub(crate) jobs: Vec<JobRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct JobRecord {
    pub(crate) job_id: String,
    pub(crate) target: String,
    pub(crate) title: String,
    pub(crate) task_class: String,
    pub(crate) model_tier: String,
    pub(crate) risk: String,
    pub(crate) context_budget_tokens: usize,
    pub(crate) path: String,
    pub(crate) result_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) context_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) auto_dag_node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) auto_dag_node_state: Option<String>,
    #[serde(default, skip_serializing_if = "skip_zero")]
    pub(crate) attempt_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) replay_commands: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) repair_commands: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) write_scope: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) required_proof: Vec<String>,
    pub(crate) tsift_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TsiftContextSummary {
    status: String,
    index_status_command: String,
    context_pack_command: String,
    source_read_command: String,
    diff_digest_command: String,
    test_digest_command: String,
    stale_fallback: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    diagnostics: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    context_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    estimated_tokens: Option<usize>,
    loaded_context_ledger: plan::LoadedContextLedger,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ContextSidecar {
    contract_version: String,
    job_id: String,
    target: String,
    command: Vec<String>,
    report: Value,
    estimated_tokens: usize,
    loaded_context_ledger: plan::LoadedContextLedger,
}

#[derive(Debug, Clone)]
struct JobTarget {
    target: String,
    action: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct JobStatusReport {
    pub(crate) parent_doc: String,
    pub(crate) cycles: Vec<CycleStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CycleStatus {
    pub(crate) cycle_id: String,
    pub(crate) created_at_unix: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) operation_doc: Option<String>,
    pub(crate) jobs: Vec<JobStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct JobStatus {
    pub(crate) job_id: String,
    pub(crate) target: String,
    pub(crate) status: String,
    pub(crate) path: String,
    pub(crate) result_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CollectReport {
    pub(crate) parent_doc: String,
    pub(crate) cycle_id: String,
    pub(crate) results: Vec<CollectedResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) missing_results: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CollectedResult {
    pub(crate) job_id: String,
    pub(crate) target: String,
    pub(crate) status: String,
    pub(crate) path: String,
    pub(crate) result: Value,
}

pub(crate) fn create(file: &Path, options: CreateOptions) -> Result<()> {
    let built = create_index(file, options)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&built).context("failed to serialize job index")?
    );
    Ok(())
}

pub(crate) fn create_for_schedule(
    file: &Path,
    schedule: &crate::auto_dag::AutoDagSchedule,
    options: CreateOptions,
) -> Result<JobsIndex> {
    if !file.exists() {
        bail!("file not found: {}", file.display());
    }
    let root = project_root(file)?;
    let canonical = file.canonicalize()?;
    let created_at = unix_now();
    let jobs_dir = root.join(".agent-doc/jobs").join(&schedule.schedule_id);
    std::fs::create_dir_all(&jobs_dir)
        .with_context(|| format!("failed to create {}", jobs_dir.display()))?;
    let source_snapshot = git_head(&root);
    let operation_doc = if options.operation_doc {
        Some(write_schedule_operation_doc(
            &root,
            file,
            schedule,
            created_at,
            &source_snapshot,
        )?)
    } else {
        None
    };

    let mut records = Vec::new();
    for (idx, node) in schedule.nodes.iter().enumerate() {
        let target = node
            .target
            .clone()
            .unwrap_or_else(|| node.id.trim_start_matches('#').to_string());
        let job_id = format!("job-{}-{:02}", target, idx + 1);
        let job_path = jobs_dir.join(format!("{job_id}.md"));
        let result_path = jobs_dir.join(format!("{job_id}.result.json"));
        let write_scope = schedule_write_scope(node);
        let required_proof = vec![
            "changed_paths".to_string(),
            "commands".to_string(),
            "verification".to_string(),
            "worker_result".to_string(),
            "confidence".to_string(),
        ];
        let record = JobRecord {
            job_id: job_id.clone(),
            target: target.clone(),
            title: format!("{} ({})", description_summary(&node.prompt), node.id),
            task_class: "auto_dag".to_string(),
            model_tier: "med".to_string(),
            risk: "medium".to_string(),
            context_budget_tokens: options.budget,
            path: relative_to(&root, &job_path),
            result_path: relative_to(&root, &result_path),
            context_path: None,
            auto_dag_node_id: Some(node.id.clone()),
            auto_dag_node_state: Some(format!("{:?}", node.state).to_ascii_lowercase()),
            attempt_count: node.attempt_count,
            replay_commands: node.replay_commands.clone(),
            repair_commands: node.repair_commands.clone(),
            write_scope: write_scope.clone(),
            required_proof: required_proof.clone(),
            tsift_status: schedule.graph_status.clone(),
        };
        let packet = render_schedule_job_packet(ScheduleJobPacketRender {
            file,
            schedule,
            node,
            job_id: &job_id,
            target: &target,
            write_scope: &write_scope,
            required_proof: &required_proof,
            context_budget: options.budget,
            source_snapshot: &source_snapshot,
            result_path: &record.result_path,
        });
        std::fs::write(&job_path, packet)
            .with_context(|| format!("failed to write {}", job_path.display()))?;
        records.push(record);
    }

    let index = JobsIndex {
        contract_version: JOB_INDEX_CONTRACT_VERSION.to_string(),
        parent_doc: relative_to(&root, file),
        parent_doc_canonical: canonical.display().to_string(),
        cycle_id: schedule.schedule_id.clone(),
        created_at_unix: created_at,
        source_snapshot,
        preserve_on_success: options.audit,
        manual_packet_only: schedule.graph_status == "missing",
        warnings: schedule.warnings.clone(),
        operation_doc,
        auto_dag_schedule_id: Some(schedule.schedule_id.clone()),
        jobs: records,
    };
    let index_path = jobs_dir.join("index.json");
    std::fs::write(
        &index_path,
        serde_json::to_string_pretty(&index).context("failed to serialize job index")?,
    )
    .with_context(|| format!("failed to write {}", index_path.display()))?;
    Ok(index)
}

fn create_index(file: &Path, options: CreateOptions) -> Result<JobsIndex> {
    if !file.exists() {
        bail!("file not found: {}", file.display());
    }

    let dispatch_plan = plan::build(file)?;
    if dispatch_plan.repo_actions.is_empty() {
        bail!("no repo actions found; add do #id work or activate a queue before creating jobs");
    }
    if !dispatch_plan.blockers.is_empty() {
        bail!(
            "plan has blocker(s); refusing to create jobs: {}",
            dispatch_plan.blockers.join("; ")
        );
    }

    let root = project_root(file)?;
    let canonical = file.canonicalize()?;
    let created_at = unix_now();
    let cycle_id = cycle_id(file, created_at)?;
    let jobs_dir = root.join(".agent-doc/jobs").join(&cycle_id);
    std::fs::create_dir_all(&jobs_dir)
        .with_context(|| format!("failed to create {}", jobs_dir.display()))?;

    let source_snapshot = git_head(&root);
    let operation_doc = if options.operation_doc {
        Some(write_operation_doc(
            &root,
            file,
            &cycle_id,
            created_at,
            &source_snapshot,
            &dispatch_plan,
        )?)
    } else {
        None
    };

    let mut records = Vec::new();
    let pending_by_id = dispatch_plan
        .pending_mutations
        .iter()
        .filter(|mutation| mutation.kind == plan::PendingMutationKind::ResolveExisting)
        .map(|mutation| (mutation.id.to_ascii_lowercase(), mutation.text.clone()))
        .collect::<BTreeMap<_, _>>();

    let job_targets = job_targets_from_plan(&dispatch_plan);
    for (idx, job_target) in job_targets.iter().enumerate() {
        let target = &job_target.target;
        let description = pending_by_id
            .get(&target.to_ascii_lowercase())
            .cloned()
            .or_else(|| job_target.action.clone())
            .unwrap_or_else(|| format!("do #{target}"));
        let write_scope = write_scope_for_target(&description, &dispatch_plan.write_scope);
        let graph_acceptance_context = graph_acceptance_context(&dispatch_plan, target)?;
        let job_id = format!("job-{}-{:02}", target, idx + 1);
        let job_path = jobs_dir.join(format!("{job_id}.md"));
        let result_path = jobs_dir.join(format!("{job_id}.result.json"));
        let context_path = jobs_dir.join(format!("{job_id}.context.json"));
        let tsift_context = collect_tsift_context(
            file,
            &root,
            &job_id,
            target,
            &dispatch_plan,
            &context_path,
            options.budget,
        )?;
        let title = format!("{} ({})", description_summary(&description), target);
        let record = JobRecord {
            job_id: job_id.clone(),
            target: target.clone(),
            title: title.clone(),
            task_class: dispatch_plan.task_class.clone(),
            model_tier: dispatch_plan.model_tier.clone(),
            risk: dispatch_plan.risk.clone(),
            context_budget_tokens: options.budget,
            path: relative_to(&root, &job_path),
            result_path: relative_to(&root, &result_path),
            context_path: tsift_context.context_path.clone(),
            auto_dag_node_id: None,
            auto_dag_node_state: None,
            attempt_count: 0,
            replay_commands: Vec::new(),
            repair_commands: Vec::new(),
            write_scope: write_scope.clone(),
            required_proof: dispatch_plan.required_proof.clone(),
            tsift_status: tsift_context.status.clone(),
        };
        let packet = render_job_packet(JobPacketRender {
            file,
            cycle_id: &cycle_id,
            job_id: &job_id,
            target,
            title: &title,
            description: &description,
            write_scope: &write_scope,
            source_snapshot: &source_snapshot,
            dispatch_plan: &dispatch_plan,
            tsift_context: &tsift_context,
            graph_acceptance_context: graph_acceptance_context.as_deref(),
            result_path: &record.result_path,
        });
        std::fs::write(&job_path, packet)
            .with_context(|| format!("failed to write {}", job_path.display()))?;
        records.push(record);
    }

    if records.is_empty() {
        bail!("no supported do #id repo actions found in plan");
    }

    let index = JobsIndex {
        contract_version: JOB_INDEX_CONTRACT_VERSION.to_string(),
        parent_doc: relative_to(&root, file),
        parent_doc_canonical: canonical.display().to_string(),
        cycle_id: cycle_id.clone(),
        created_at_unix: created_at,
        source_snapshot,
        preserve_on_success: options.audit,
        manual_packet_only: dispatch_plan.manual_packet_only,
        warnings: dispatch_plan.warnings.clone(),
        operation_doc,
        auto_dag_schedule_id: None,
        jobs: records,
    };
    let index_path = jobs_dir.join("index.json");
    std::fs::write(
        &index_path,
        serde_json::to_string_pretty(&index).context("failed to serialize job index")?,
    )
    .with_context(|| format!("failed to write {}", index_path.display()))?;
    Ok(index)
}

pub(crate) fn list(file: &Path, json: bool) -> Result<()> {
    let report = status_report(file)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    for cycle in report.cycles {
        println!("{} ({})", cycle.cycle_id, cycle.created_at_unix);
        for job in cycle.jobs {
            println!("  {} #{} {}", job.job_id, job.target, job.path);
        }
    }
    Ok(())
}

pub(crate) fn status(file: &Path, json: bool) -> Result<()> {
    let report = status_report(file)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    for cycle in report.cycles {
        println!("{} ({})", cycle.cycle_id, cycle.created_at_unix);
        for job in cycle.jobs {
            println!(
                "  {} #{} status={} result={}",
                job.job_id, job.target, job.status, job.result_path
            );
        }
    }
    Ok(())
}

pub(crate) fn collect(file: &Path, cycle: Option<&str>, json: bool) -> Result<()> {
    let root = project_root(file)?;
    let indexes = read_indexes_for(file)?;
    let Some(index) = select_index(indexes, cycle) else {
        bail!("no job cycles found for {}", file.display());
    };
    let mut results = Vec::new();
    let mut missing = Vec::new();
    for job in &index.jobs {
        let result_path = root.join(&job.result_path);
        if let Some(value) = read_result_json(&result_path)? {
            let status = value
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string();
            results.push(CollectedResult {
                job_id: job.job_id.clone(),
                target: job.target.clone(),
                status,
                path: job.result_path.clone(),
                result: value,
            });
            continue;
        }
        let packet_path = root.join(&job.path);
        if let Some(value) = read_embedded_worker_result(&packet_path)? {
            let status = value
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string();
            results.push(CollectedResult {
                job_id: job.job_id.clone(),
                target: job.target.clone(),
                status,
                path: job.path.clone(),
                result: value,
            });
        } else {
            missing.push(job.job_id.clone());
        }
    }
    let report = CollectReport {
        parent_doc: index.parent_doc,
        cycle_id: index.cycle_id,
        results,
        missing_results: missing,
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("cycle {}", report.cycle_id);
        for result in &report.results {
            println!(
                "  {} #{} status={}",
                result.job_id, result.target, result.status
            );
        }
        for job_id in &report.missing_results {
            println!("  {} status=missing", job_id);
        }
    }
    Ok(())
}

fn status_report(file: &Path) -> Result<JobStatusReport> {
    let root = project_root(file)?;
    let indexes = read_indexes_for(file)?;
    let mut cycles = Vec::new();
    for index in indexes {
        let jobs = index
            .jobs
            .iter()
            .map(|job| {
                let result_path = root.join(&job.result_path);
                let packet_path = root.join(&job.path);
                let status = if result_path.exists() {
                    "result_sidecar"
                } else if read_embedded_worker_result(&packet_path)
                    .ok()
                    .flatten()
                    .is_some()
                {
                    "embedded_result"
                } else {
                    "open"
                };
                JobStatus {
                    job_id: job.job_id.clone(),
                    target: job.target.clone(),
                    status: status.to_string(),
                    path: job.path.clone(),
                    result_path: job.result_path.clone(),
                }
            })
            .collect();
        cycles.push(CycleStatus {
            cycle_id: index.cycle_id,
            created_at_unix: index.created_at_unix,
            operation_doc: index.operation_doc,
            jobs,
        });
    }
    Ok(JobStatusReport {
        parent_doc: relative_to(&root, file),
        cycles,
    })
}

fn read_indexes_for(file: &Path) -> Result<Vec<JobsIndex>> {
    let root = project_root(file)?;
    let canonical = file.canonicalize()?.display().to_string();
    let jobs_root = root.join(".agent-doc/jobs");
    let mut indexes = Vec::new();
    if !jobs_root.exists() {
        return Ok(indexes);
    }
    for entry in std::fs::read_dir(&jobs_root)
        .with_context(|| format!("failed to read {}", jobs_root.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let index_path = entry.path().join("index.json");
        if !index_path.exists() {
            continue;
        }
        let text = std::fs::read_to_string(&index_path)
            .with_context(|| format!("failed to read {}", index_path.display()))?;
        let index: JobsIndex = serde_json::from_str(&text)
            .with_context(|| format!("failed to parse {}", index_path.display()))?;
        if index.parent_doc_canonical == canonical {
            indexes.push(index);
        }
    }
    indexes.sort_by_key(|index| index.created_at_unix);
    Ok(indexes)
}

fn select_index(mut indexes: Vec<JobsIndex>, cycle: Option<&str>) -> Option<JobsIndex> {
    if let Some(cycle) = cycle {
        return indexes.into_iter().find(|index| index.cycle_id == cycle);
    }
    indexes.pop()
}

fn read_result_json(path: &Path) -> Result<Option<Value>> {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let value: Value = serde_json::from_str(&text)
                .with_context(|| format!("failed to parse worker result {}", path.display()))?;
            validate_worker_result_contract(path, &value)?;
            Ok(Some(value))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("failed to read {}", path.display())),
    }
}

fn read_embedded_worker_result(path: &Path) -> Result<Option<Value>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).with_context(|| format!("failed to read {}", path.display())),
    };
    let Some(section) = text.split("## Worker Result").nth(1) else {
        return Ok(None);
    };
    let mut in_json = false;
    let mut body = String::new();
    for line in section.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            if in_json {
                break;
            }
            if trimmed == "```json" || trimmed == "```" {
                in_json = true;
            }
            continue;
        }
        if in_json {
            body.push_str(line);
            body.push('\n');
        }
    }
    if body.trim().is_empty() {
        return Ok(None);
    }
    let value = serde_json::from_str(body.trim())
        .with_context(|| format!("failed to parse embedded worker result {}", path.display()))?;
    validate_worker_result_contract(path, &value)?;
    Ok(Some(value))
}

fn job_targets_from_plan(dispatch_plan: &plan::DispatchPlan) -> Vec<JobTarget> {
    let mut seen = BTreeSet::new();
    let mut targets = Vec::new();

    for action in &dispatch_plan.repo_actions {
        for target in crate::tsift_graph::extract_do_targets(action) {
            if seen.insert(target.to_ascii_lowercase()) {
                targets.push(JobTarget {
                    target,
                    action: Some(action.clone()),
                });
            }
        }
    }

    for mutation in dispatch_plan
        .pending_mutations
        .iter()
        .filter(|mutation| mutation.kind == plan::PendingMutationKind::ResolveExisting)
    {
        if seen.insert(mutation.id.to_ascii_lowercase()) {
            targets.push(JobTarget {
                target: mutation.id.clone(),
                action: None,
            });
        }
    }

    targets
}

fn write_scope_for_target(description: &str, default_scope: &[String]) -> Vec<String> {
    let labeled = extract_labeled_path_refs(description);
    if !labeled.is_empty() {
        return labeled;
    }

    let paths = extract_path_refs(description);
    if !paths.is_empty() {
        return paths;
    }

    default_scope.to_vec()
}

fn extract_labeled_path_refs(text: &str) -> Vec<String> {
    let tokens = text.split_whitespace().collect::<Vec<_>>();
    let mut paths = Vec::new();
    for (idx, token) in tokens.iter().enumerate() {
        let label = token
            .trim_matches(|ch: char| {
                matches!(
                    ch,
                    '`' | '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
                )
            })
            .to_ascii_lowercase();
        if !matches!(
            label.as_str(),
            "scope:" | "write_scope:" | "write-scope:" | "write" | "write_scope"
        ) {
            continue;
        }
        for candidate in tokens.iter().skip(idx + 1).take(4) {
            if let Some(path) = normalize_path_candidate(candidate) {
                push_unique(&mut paths, path);
                break;
            }
        }
    }
    paths
}

fn extract_path_refs(text: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for token in text.split_whitespace() {
        if let Some(path) = normalize_path_candidate(token) {
            push_unique(&mut paths, path);
        }
    }
    paths
}

fn normalize_path_candidate(token: &str) -> Option<String> {
    let mut candidate = token
        .trim_matches(|ch: char| {
            matches!(
                ch,
                '`' | '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
            )
        })
        .trim_start_matches("](")
        .trim_start_matches('<')
        .trim_end_matches('>')
        .to_string();
    while candidate.ends_with(|ch: char| {
        matches!(
            ch,
            '.' | ',' | ';' | ':' | ')' | ']' | '}' | '`' | '"' | '\''
        )
    }) {
        candidate.pop();
    }
    if let Some((path, line)) = candidate.rsplit_once(':')
        && !path.is_empty()
        && line.chars().all(|ch| ch.is_ascii_digit())
    {
        candidate = path.to_string();
    }
    if candidate.is_empty()
        || candidate.starts_with('/')
        || candidate.starts_with('#')
        || candidate.contains("://")
        || !candidate.contains('/')
    {
        return None;
    }
    if !candidate
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | '+' | '@'))
    {
        return None;
    }
    Some(candidate)
}

fn push_unique(items: &mut Vec<String>, item: String) {
    if !items.iter().any(|existing| existing == &item) {
        items.push(item);
    }
}

fn graph_acceptance_context(
    dispatch_plan: &plan::DispatchPlan,
    target: &str,
) -> Result<Option<String>> {
    let Some(graph_evidence) = &dispatch_plan.graph_evidence else {
        return Ok(None);
    };
    let task_label = format!("do #{target}");
    let context = graph_evidence
        .prompt_context_for_task(&task_label)?
        .with_context(|| format!("tsift graph evidence missing target #{target}"))?;
    if !context.contains("\"lower_agent_job_packet\"") {
        bail!("tsift graph evidence for #{target} missing lower_agent_job_packet");
    }
    Ok(Some(context))
}

fn validate_worker_result_contract(path: &Path, value: &Value) -> Result<()> {
    let mut errors = Vec::new();
    require_json_string(
        &mut errors,
        value,
        "contract_version",
        Some(WORKER_RESULT_CONTRACT_VERSION),
    );
    require_json_string(&mut errors, value, "status", None);
    if let Some(status) = value.get("status").and_then(Value::as_str)
        && !matches!(status, "complete" | "blocked" | "escalate")
    {
        errors.push(format!("unsupported status `{status}`"));
    }
    for field in [
        "changed_paths",
        "commands_run",
        "touched_files",
        "expected_tests",
        "follow_up_ids",
        "findings",
        "proof",
        "needs_parent_attention",
    ] {
        require_json_array(&mut errors, value, field);
    }
    require_json_string(&mut errors, value, "confidence", None);

    if errors.is_empty() {
        Ok(())
    } else {
        bail!(
            "worker result {} violates {}: {}",
            path.display(),
            WORKER_RESULT_CONTRACT_VERSION,
            errors.join("; ")
        )
    }
}

fn require_json_string(
    errors: &mut Vec<String>,
    value: &Value,
    field: &str,
    expected: Option<&str>,
) {
    match value.get(field).and_then(Value::as_str) {
        Some(actual) if expected.is_none_or(|expected| expected == actual) => {}
        Some(actual) => errors.push(format!("{field}={actual}, expected {}", expected.unwrap())),
        None => errors.push(format!("missing string field {field}")),
    }
}

fn require_json_array(errors: &mut Vec<String>, value: &Value, field: &str) {
    match value.get(field) {
        Some(Value::Array(_)) => {}
        Some(_) => errors.push(format!("{field} must be an array")),
        None => errors.push(format!("missing array field {field}")),
    }
}

struct JobPacketRender<'a> {
    file: &'a Path,
    cycle_id: &'a str,
    job_id: &'a str,
    target: &'a str,
    title: &'a str,
    description: &'a str,
    write_scope: &'a [String],
    source_snapshot: &'a str,
    dispatch_plan: &'a plan::DispatchPlan,
    tsift_context: &'a TsiftContextSummary,
    graph_acceptance_context: Option<&'a str>,
    result_path: &'a str,
}

fn render_job_packet(input: JobPacketRender<'_>) -> String {
    let write_scope = yaml_list(input.write_scope);
    let required_proof = yaml_list(&input.dispatch_plan.required_proof);
    let tsift_json =
        serde_json::to_string_pretty(input.tsift_context).unwrap_or_else(|_| "{}".to_string());
    let graph_acceptance = input
        .graph_acceptance_context
        .map(|context| format!("\n## tsift Graph Acceptance Gate\n\n```text\n{context}\n```\n"))
        .unwrap_or_default();
    let manual_packet_warning = render_manual_packet_warning(input.dispatch_plan);
    format!(
        r#"---
contract_version: {job_contract}
parent_doc: {parent_doc}
cycle_id: {cycle_id}
job_id: {job_id}
prompt_target: {target}
task_class: {task_class}
model_tier: {model_tier}
risk: {risk}
write_scope:
{write_scope}
context_budget_tokens: {context_budget}
source_snapshot: {source_snapshot}
tsift_index: {tsift_status}
manual_packet_only: {manual_packet_only}
result_path: {result_path}
---

# Job Packet: {title}

## Goal

Complete `#{target}` only: {description}

## Allowed Commands

- Read files inside the declared write scope and read-only tsift context.
- Run focused tests or build commands needed to prove this job.
- Do not edit outside `write_scope`; escalate instead.

## Required Context

- Parent document: `{parent_doc}`
- Source snapshot: `{source_snapshot}`
- Task class: `{task_class}`
- Risk: `{risk}`
- Required proof:
{required_proof}

## tsift Handles

```json
{tsift_json}
```
{graph_acceptance}
{manual_packet_warning}

## Acceptance Criteria

- The job goal is complete or explicitly blocked.
- Changed paths stay inside `write_scope`.
- Verification commands and their outcomes are listed.
- Touched files, expected tests, and follow-up ids are present in the worker result, even when empty.
- Any stale/missing tsift context is called out in `needs_parent_attention`.

## Output Schema

Save a JSON object to `{result_path}` or paste it under `## Worker Result`:

```json
{{
  "contract_version": "{worker_contract}",
  "status": "complete|blocked|escalate",
  "changed_paths": [],
  "commands_run": [],
  "touched_files": [],
  "expected_tests": [],
  "follow_up_ids": [],
  "findings": [],
  "proof": [],
  "confidence": "low|medium|high",
  "needs_parent_attention": []
}}
```

## Escalation Conditions

- Required context is stale, missing, contradictory, or outside this packet.
- The task needs files outside `write_scope`.
- Verification fails in a way that is not local to this job.
- The implementation affects concurrency, routing, security, git closeout, or session document invariants beyond this packet.

## Worker Result

```json
```
"#,
        job_contract = JOB_PACKET_CONTRACT_VERSION,
        parent_doc = input.file.display(),
        cycle_id = input.cycle_id,
        job_id = input.job_id,
        target = input.target,
        task_class = input.dispatch_plan.task_class,
        model_tier = input.dispatch_plan.model_tier,
        risk = input.dispatch_plan.risk,
        write_scope = write_scope,
        context_budget = input.dispatch_plan.context_budget_tokens,
        source_snapshot = input.source_snapshot,
        tsift_status = input.tsift_context.status,
        manual_packet_only = input.dispatch_plan.manual_packet_only,
        result_path = input.result_path,
        title = input.title,
        description = input.description,
        required_proof = required_proof,
        tsift_json = tsift_json,
        graph_acceptance = graph_acceptance,
        manual_packet_warning = manual_packet_warning,
        worker_contract = WORKER_RESULT_CONTRACT_VERSION,
    )
}

struct ScheduleJobPacketRender<'a> {
    file: &'a Path,
    schedule: &'a crate::auto_dag::AutoDagSchedule,
    node: &'a crate::auto_dag::AutoDagNode,
    job_id: &'a str,
    target: &'a str,
    write_scope: &'a [String],
    required_proof: &'a [String],
    context_budget: usize,
    source_snapshot: &'a str,
    result_path: &'a str,
}

fn render_schedule_job_packet(input: ScheduleJobPacketRender<'_>) -> String {
    let write_scope = yaml_list(input.write_scope);
    let required_proof = yaml_list(input.required_proof);
    let deps = yaml_list(&input.node.deps);
    let replay_commands = yaml_list(&input.node.replay_commands);
    let repair_commands = yaml_list(&input.node.repair_commands);
    format!(
        r#"---
contract_version: {job_contract}
parent_doc: {parent_doc}
cycle_id: {schedule_id}
job_id: {job_id}
prompt_target: {target}
task_class: auto_dag
model_tier: med
risk: medium
write_scope:
{write_scope}
context_budget_tokens: {context_budget}
source_snapshot: {source_snapshot}
tsift_index: {graph_status}
manual_packet_only: {manual_packet_only}
auto_dag_schedule_id: {schedule_id}
auto_dag_node_id: {node_id}
auto_dag_node_state: {node_state}
attempt_count: {attempt_count}
result_path: {result_path}
---

# Job Packet: auto-DAG {node_id}

## Goal

Complete `{node_id}` only: {prompt}

## DAG Schedule

- Schedule id: `{schedule_id}`
- Source: `{source_kind}`
- Batch deps:
{deps}
- Current state: `{node_state}`
- Attempt count: {attempt_count}

## Allowed Commands

- Read files inside the declared write scope and read-only context.
- Run focused tests or build commands needed to prove this node.
- Do not edit outside `write_scope`; write a blocked worker result instead.

## Required Context

- Parent document: `{parent_doc}`
- Source snapshot: `{source_snapshot}`
- Required proof:
{required_proof}

## Replay And Repair

Replay commands:
{replay_commands}

Repair commands:
{repair_commands}

## Acceptance Criteria

- The DAG node is complete or explicitly blocked.
- Changed paths stay inside `write_scope`.
- Verification commands and outcomes are listed.
- Worker result fields are present even when arrays are empty.
- Downstream nodes must not be launched by a worker.

## Output Schema

Save a JSON object to `{result_path}` or paste it under `## Worker Result`:

```json
{{
  "contract_version": "{worker_contract}",
  "status": "complete|blocked|escalate",
  "changed_paths": [],
  "commands_run": [],
  "touched_files": [],
  "expected_tests": [],
  "follow_up_ids": [],
  "findings": [],
  "proof": [],
  "confidence": "low|medium|high",
  "needs_parent_attention": []
}}
```

## Escalation Conditions

- Required graph/context evidence is stale, missing, contradictory, or outside this packet.
- The task needs files outside `write_scope`.
- Verification fails outside this node's scope.
- The implementation affects concurrency, routing, security, git closeout, or session document invariants beyond this packet.

## Worker Result

```json
```
"#,
        job_contract = JOB_PACKET_CONTRACT_VERSION,
        parent_doc = input.file.display(),
        schedule_id = input.schedule.schedule_id,
        job_id = input.job_id,
        target = input.target,
        write_scope = write_scope,
        context_budget = input.context_budget,
        source_snapshot = input.source_snapshot,
        graph_status = input.schedule.graph_status,
        manual_packet_only = input.schedule.graph_status == "missing",
        node_id = input.node.id,
        node_state = format!("{:?}", input.node.state).to_ascii_lowercase(),
        attempt_count = input.node.attempt_count,
        result_path = input.result_path,
        prompt = input.node.prompt,
        source_kind = input.schedule.source_kind,
        deps = deps,
        required_proof = required_proof,
        replay_commands = replay_commands,
        repair_commands = repair_commands,
        worker_contract = WORKER_RESULT_CONTRACT_VERSION,
    )
}

fn schedule_write_scope(node: &crate::auto_dag::AutoDagNode) -> Vec<String> {
    let labeled = extract_labeled_path_refs(&node.prompt);
    if !labeled.is_empty() {
        return labeled;
    }
    let paths = extract_path_refs(&node.prompt);
    if !paths.is_empty() {
        return paths;
    }
    vec!["undetermined".to_string()]
}

fn render_manual_packet_warning(dispatch_plan: &plan::DispatchPlan) -> String {
    if !dispatch_plan.manual_packet_only {
        return String::new();
    }

    let warnings = if dispatch_plan.warnings.is_empty() {
        "- Graph acceptance evidence is unavailable; parent review must treat this as a manual packet.".to_string()
    } else {
        dispatch_plan
            .warnings
            .iter()
            .map(|warning| format!("- {warning}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        "\n## Manual Packet Warning\n\nmanual_packet_only=true. Graph acceptance evidence was not attached.\n\n{warnings}\n"
    )
}

fn collect_tsift_context(
    file: &Path,
    root: &Path,
    job_id: &str,
    target: &str,
    dispatch_plan: &plan::DispatchPlan,
    context_path: &Path,
    budget: usize,
) -> Result<TsiftContextSummary> {
    let mut summary = TsiftContextSummary {
        status: dispatch_plan.tsift_context.status.clone(),
        index_status_command: dispatch_plan.tsift_context.index_status_command.clone(),
        context_pack_command: dispatch_plan.tsift_context.context_pack_command.clone(),
        source_read_command: dispatch_plan.tsift_context.source_read_command.clone(),
        diff_digest_command: dispatch_plan.tsift_context.diff_digest_command.clone(),
        test_digest_command: dispatch_plan.tsift_context.test_digest_command.clone(),
        stale_fallback: dispatch_plan.tsift_context.stale_fallback.clone(),
        diagnostics: Vec::new(),
        context_path: None,
        estimated_tokens: None,
        loaded_context_ledger: dispatch_plan.tsift_context.loaded_context_ledger.clone(),
    };

    let bin = std::env::var("AGENT_DOC_TSIFT_BIN").unwrap_or_else(|_| "tsift".to_string());
    let status = Command::new(&bin)
        .args(["status", "--json"])
        .arg(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();
    match status {
        Ok(output) if output.status.success() => {
            let status_stdout = String::from_utf8_lossy(&output.stdout);
            let status_stderr = String::from_utf8_lossy(&output.stderr);
            if let Ok(value) = serde_json::from_slice::<Value>(&output.stdout)
                && let Some(index_status) = value
                    .get("index")
                    .and_then(|index| index.get("status"))
                    .and_then(Value::as_str)
            {
                summary.status = index_status.to_string();
            }
            summary.loaded_context_ledger =
                plan::build_loaded_context_ledger(vec![plan::loaded_context_record(
                    "agent-doc.job.tsift-status",
                    "generated-state",
                    &root.display().to_string(),
                    &format!("{status_stdout}{status_stderr}"),
                    None,
                    "job_context_collection",
                    "verify tsift index status before materializing context sidecar",
                )]);
        }
        Ok(output) => {
            summary.status = "unavailable".to_string();
            summary.diagnostics.push(format!(
                "`{} status --json {}` exited with {}: {}{}{}",
                bin,
                root.display(),
                output.status,
                String::from_utf8_lossy(&output.stderr).trim(),
                if output.stderr.is_empty() || output.stdout.is_empty() {
                    ""
                } else {
                    "\n"
                },
                String::from_utf8_lossy(&output.stdout).trim()
            ));
            return Ok(summary);
        }
        Err(err) => {
            summary.status = "unavailable".to_string();
            summary
                .diagnostics
                .push(format!("failed to launch `{bin}` for tsift status: {err}"));
            return Ok(summary);
        }
    }

    let output = Command::new(&bin)
        .args(["context-pack"])
        .arg(file)
        .args(["--json", "--budget", "normal", "--max-bytes"])
        .arg(budget.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();
    match output {
        Ok(output) if output.status.success() => {
            let report: Value = serde_json::from_slice(&output.stdout)
                .context("failed to parse tsift context-pack JSON")?;
            let bytes = output.stdout.len();
            let mut ledger_entries = summary.loaded_context_ledger.entries.clone();
            ledger_entries.push(plan::loaded_context_record(
                "agent-doc.job.context-pack",
                "generated-state",
                &file.display().to_string(),
                &String::from_utf8_lossy(&output.stdout),
                None,
                "job_context_collection",
                "materialize bounded tsift context-pack sidecar",
            ));
            let loaded_context_ledger = plan::build_loaded_context_ledger(ledger_entries);
            let sidecar = ContextSidecar {
                contract_version: "agent-doc-tsift-context-sidecar-v1".to_string(),
                job_id: job_id.to_string(),
                target: target.to_string(),
                command: vec![
                    bin,
                    "context-pack".to_string(),
                    file.display().to_string(),
                    "--json".to_string(),
                    "--budget".to_string(),
                    "normal".to_string(),
                    "--max-bytes".to_string(),
                    budget.to_string(),
                ],
                report,
                estimated_tokens: bytes.div_ceil(4),
                loaded_context_ledger: loaded_context_ledger.clone(),
            };
            std::fs::write(
                context_path,
                serde_json::to_string_pretty(&sidecar)
                    .context("failed to serialize tsift context sidecar")?,
            )
            .with_context(|| format!("failed to write {}", context_path.display()))?;
            summary.context_path = Some(relative_to(root, context_path));
            summary.estimated_tokens = Some(sidecar.estimated_tokens);
            summary.loaded_context_ledger = loaded_context_ledger;
        }
        Ok(output) => {
            summary.diagnostics.push(format!(
                "`tsift context-pack` exited with {}: {}{}{}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim(),
                if output.stderr.is_empty() || output.stdout.is_empty() {
                    ""
                } else {
                    "\n"
                },
                String::from_utf8_lossy(&output.stdout).trim()
            ));
        }
        Err(err) => {
            summary
                .diagnostics
                .push(format!("failed to launch tsift context-pack: {err}"));
        }
    }
    Ok(summary)
}

fn write_operation_doc(
    root: &Path,
    file: &Path,
    cycle_id: &str,
    created_at: u64,
    source_snapshot: &str,
    dispatch_plan: &plan::DispatchPlan,
) -> Result<String> {
    let dir = if root.join("tasks/agent-doc").is_dir() {
        root.join("tasks/agent-doc/operations")
    } else {
        root.join(".agent-doc/operations")
    };
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
    let path = dir.join(format!("{cycle_id}.md"));
    let warnings = yaml_list(&dispatch_plan.warnings);
    let content = format!(
        r#"# Operation: {cycle_id}

parent_doc: {parent_doc}
created_at_unix: {created_at}
source_snapshot: {source_snapshot}
task_class: {task_class}
risk: {risk}
dispatch_mode: {dispatch_mode}

## Planning Decision

- dispatch_candidate: {dispatch_candidate}
- parallelizable: {parallelizable}
- suggested_parent_tier: {suggested_parent_tier}
- context_budget_tokens: {context_budget}
- manual_packet_only: {manual_packet_only}
- warnings:
{warnings}

## Job Packet List

Run `agent-doc jobs list {parent_doc}` to inspect generated packets.

## Worker Results

Run `agent-doc jobs collect {parent_doc} --cycle {cycle_id}` after workers complete.

## Verification Evidence

Parent review must run required verification before `agent-doc finalize`.
"#,
        cycle_id = cycle_id,
        parent_doc = file.display(),
        created_at = created_at,
        source_snapshot = source_snapshot,
        task_class = dispatch_plan.task_class,
        risk = dispatch_plan.risk,
        dispatch_mode = dispatch_plan.dispatch_mode,
        dispatch_candidate = dispatch_plan.dispatch_candidate,
        parallelizable = dispatch_plan.parallelizable,
        suggested_parent_tier = dispatch_plan.suggested_parent_tier,
        context_budget = dispatch_plan.context_budget_tokens,
        manual_packet_only = dispatch_plan.manual_packet_only,
        warnings = warnings,
    );
    std::fs::write(&path, content)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(relative_to(root, &path))
}

fn write_schedule_operation_doc(
    root: &Path,
    file: &Path,
    schedule: &crate::auto_dag::AutoDagSchedule,
    created_at: u64,
    source_snapshot: &str,
) -> Result<String> {
    let dir = if root.join("tasks/agent-doc").is_dir() {
        root.join("tasks/agent-doc/operations")
    } else {
        root.join(".agent-doc/operations")
    };
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
    let path = dir.join(format!("{}.md", schedule.schedule_id));
    let nodes = schedule
        .nodes
        .iter()
        .map(|node| {
            format!(
                "- {} state={:?} attempts={} deps=[{}]",
                node.id,
                node.state,
                node.attempt_count,
                node.deps.join(",")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let batches = schedule
        .batches
        .iter()
        .enumerate()
        .map(|(idx, batch)| format!("- batch {}: {}", idx + 1, batch.join(", ")))
        .collect::<Vec<_>>()
        .join("\n");
    let guard = serde_json::to_string_pretty(&schedule.guard)
        .unwrap_or_else(|_| "{\"action\":\"unknown\"}".to_string());
    let content = format!(
        r#"# Auto-DAG Operation: {schedule_id}

parent_doc: {parent_doc}
created_at_unix: {created_at}
source_snapshot: {source_snapshot}
source_kind: {source_kind}
graph_status: {graph_status}

## Guard

```json
{guard}
```

## Batches

{batches}

## Nodes

{nodes}

## Replay

Run `agent-doc orchestrate {parent_doc} --mode dag --resume-schedule {schedule_id}` to resume this schedule.
"#,
        schedule_id = schedule.schedule_id,
        parent_doc = file.display(),
        created_at = created_at,
        source_snapshot = source_snapshot,
        source_kind = schedule.source_kind,
        graph_status = schedule.graph_status,
        guard = guard,
        batches = batches,
        nodes = nodes,
    );
    std::fs::write(&path, content)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(relative_to(root, &path))
}

fn project_root(file: &Path) -> Result<PathBuf> {
    let canonical = file.canonicalize()?;
    agent_doc_fs::find_project_root(&canonical)
        .with_context(|| format!("could not find .agent-doc root for {}", file.display()))
}

fn cycle_id(file: &Path, created_at: u64) -> Result<String> {
    let hash = snapshot::doc_hash(file)?;
    Ok(format!("cycle-{created_at}-{}", &hash[..8]))
}

fn unix_now() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs(),
        Err(_) => 0,
    }
}

fn git_head(root: &Path) -> String {
    match Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
    {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
        Ok(output) => format!(
            "unknown (git rev-parse exited with {}: {})",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ),
        Err(err) => format!("unknown (failed to launch git: {err})"),
    }
}

fn description_summary(text: &str) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX: usize = 72;
    if normalized.chars().count() <= MAX {
        normalized
    } else {
        let mut out = normalized.chars().take(MAX).collect::<String>();
        out.push_str("...");
        out
    }
}

fn yaml_list(items: &[String]) -> String {
    if items.is_empty() {
        return "  []".to_string();
    }
    items
        .iter()
        .map(|item| format!("  - {}", item))
        .collect::<Vec<_>>()
        .join("\n")
}

fn relative_to(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_orchestration::snapshot;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
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

    fn setup_doc() -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
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

<!-- agent:backlog -->
- [ ] [#docs1] Add job packet docs and tests.
- [ ] [#spec2] Add job packet spec.
<!-- /agent:backlog -->
"#;
        let current = baseline.replace(
            "<!-- /agent:exchange -->",
            "do [#docs1]\ndo [#spec2]\n<!-- /agent:exchange -->",
        );
        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();
        (dir, doc)
    }

    #[test]
    fn create_generates_packets_index_and_operation_doc() {
        let (dir, doc) = setup_doc();
        let _env = EnvGuard::set("AGENT_DOC_TSIFT_BIN", "/no/such/tsift");

        let index = create_index(
            &doc,
            CreateOptions {
                operation_doc: true,
                audit: true,
                budget: 1024,
            },
        )
        .unwrap();

        assert_eq!(index.contract_version, JOB_INDEX_CONTRACT_VERSION);
        assert_eq!(index.jobs.len(), 2);
        assert!(index.preserve_on_success);
        assert!(index.operation_doc.is_some());
        for job in &index.jobs {
            let path = dir.path().join(&job.path);
            let text = std::fs::read_to_string(path).unwrap();
            assert!(text.contains(JOB_PACKET_CONTRACT_VERSION));
            assert!(text.contains(WORKER_RESULT_CONTRACT_VERSION));
            assert!(text.contains("failed to launch"));
            assert_eq!(job.tsift_status, "unavailable");
        }
    }

    #[test]
    fn create_allows_manual_packets_when_graph_db_is_locked() {
        let (dir, doc) = setup_doc();
        std::fs::create_dir_all(dir.path().join(".tsift")).unwrap();
        std::fs::write(dir.path().join(".tsift/graph.db"), "fake").unwrap();
        let fake = dir.path().join("fake-tsift-lock");
        let mut script = std::fs::File::create(&fake).unwrap();
        writeln!(
            script,
            r#"#!/bin/sh
case "$*" in
  *"graph-db"*"--json status"*)
    echo 'Error code 5: The database file is locked' >&2
    exit 1
    ;;
  *"status"*"--json"*)
    printf '{{"index":{{"status":"fresh"}}}}'
    ;;
  *"context-pack"*)
    printf '{{"summary":"manual packet"}}'
    ;;
  *)
    echo "unexpected fake tsift args: $*" >&2
    exit 2
    ;;
esac
"#
        )
        .unwrap();
        drop(script);
        let mut perms = std::fs::metadata(&fake).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&fake, perms).unwrap();
        let _env = EnvGuard::set("AGENT_DOC_TSIFT_BIN", fake.to_str().unwrap());

        let index = create_index(
            &doc,
            CreateOptions {
                operation_doc: true,
                audit: true,
                budget: 1024,
            },
        )
        .unwrap();

        assert!(index.manual_packet_only);
        assert!(
            index
                .warnings
                .iter()
                .any(|warning| warning.contains("database file is locked")),
            "expected lock warning, got {:?}",
            index.warnings
        );
        assert_eq!(index.jobs.len(), 2);
        let packet = std::fs::read_to_string(dir.path().join(&index.jobs[0].path)).unwrap();
        assert!(packet.contains("manual_packet_only: true"));
        assert!(packet.contains("## Manual Packet Warning"));
        assert!(packet.contains("database file is locked"));
        let operation_doc = index.operation_doc.as_ref().unwrap();
        let operation_text = std::fs::read_to_string(dir.path().join(operation_doc)).unwrap();
        assert!(operation_text.contains("- manual_packet_only: true"));
    }

    #[test]
    fn create_expands_compound_do_directive_and_derives_target_write_scope() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
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

<!-- agent:backlog -->
- [ ] [#x63e] Implement the knowledge layer (scope: `build-party/specs/convex-knowledge-layer.md`) with tests.
- [ ] [#v4v0] Add the session-share route packet.
<!-- /agent:backlog -->
"#;
        let current = baseline.replace(
            "<!-- /agent:exchange -->",
            "do [#x63e] [#v4v0]. spec-test-build-install-commit-push\n<!-- /agent:exchange -->",
        );
        std::fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();
        let _env = EnvGuard::set("AGENT_DOC_TSIFT_BIN", "/no/such/tsift");

        let index = create_index(
            &doc,
            CreateOptions {
                operation_doc: false,
                audit: false,
                budget: 512,
            },
        )
        .unwrap();

        assert_eq!(
            index
                .jobs
                .iter()
                .map(|job| job.target.as_str())
                .collect::<Vec<_>>(),
            vec!["x63e", "v4v0"]
        );
        assert_eq!(
            index.jobs[0].write_scope,
            vec!["build-party/specs/convex-knowledge-layer.md"]
        );
        let packet = std::fs::read_to_string(dir.path().join(&index.jobs[0].path)).unwrap();
        assert!(packet.contains("build-party/specs/convex-knowledge-layer.md"));
    }

    #[test]
    fn collect_reads_result_sidecars() {
        let (dir, doc) = setup_doc();
        let _env = EnvGuard::set("AGENT_DOC_TSIFT_BIN", "/no/such/tsift");
        let index = create_index(
            &doc,
            CreateOptions {
                operation_doc: false,
                audit: false,
                budget: 512,
            },
        )
        .unwrap();
        let first = &index.jobs[0];
        let result_path = dir.path().join(&first.result_path);
        std::fs::write(
            &result_path,
            format!(
                r#"{{"contract_version":"{}","status":"complete","changed_paths":[],"commands_run":[],"findings":[],"proof":[],"confidence":"high","needs_parent_attention":[]}}"#,
                WORKER_RESULT_CONTRACT_VERSION
            ),
        )
        .unwrap();

        let err = read_result_json(&result_path).unwrap_err().to_string();
        assert!(err.contains("missing array field touched_files"));

        std::fs::write(
            &result_path,
            format!(
                r#"{{"contract_version":"{}","status":"complete","changed_paths":[],"commands_run":[],"touched_files":[],"expected_tests":[],"follow_up_ids":[],"findings":[],"proof":[],"confidence":"high","needs_parent_attention":[]}}"#,
                WORKER_RESULT_CONTRACT_VERSION
            ),
        )
        .unwrap();

        let indexes = read_indexes_for(&doc).unwrap();
        let selected = select_index(indexes, Some(&index.cycle_id)).unwrap();
        assert_eq!(selected.cycle_id, index.cycle_id);
        assert!(read_result_json(&result_path).unwrap().is_some());
    }

    #[test]
    fn create_writes_tsift_context_sidecar_when_command_succeeds() {
        let (dir, doc) = setup_doc();
        let fake = dir.path().join("fake-tsift");
        let mut script = std::fs::File::create(&fake).unwrap();
        writeln!(
            script,
            "#!/bin/sh\nif [ \"$1\" = status ]; then printf '{{\"index\":{{\"status\":\"fresh\"}}}}'; else printf '{{\"summary\":\"ok\"}}'; fi"
        )
        .unwrap();
        drop(script);
        let mut perms = std::fs::metadata(&fake).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&fake, perms).unwrap();
        let _env = EnvGuard::set("AGENT_DOC_TSIFT_BIN", fake.to_str().unwrap());

        let index = create_index(
            &doc,
            CreateOptions {
                operation_doc: false,
                audit: false,
                budget: 512,
            },
        )
        .unwrap();

        let context = index.jobs[0].context_path.as_ref().unwrap();
        let sidecar = std::fs::read_to_string(dir.path().join(context)).unwrap();
        assert!(sidecar.contains("agent-doc-tsift-context-sidecar-v1"));
        assert!(sidecar.contains("loaded_context_ledger"));
        assert!(sidecar.contains("agent-doc.job.context-pack"));
        assert!(sidecar.contains("duplicate_expansions_suppressed"));
        assert_eq!(index.jobs[0].tsift_status, "fresh");
    }

    #[test]
    fn create_for_schedule_writes_dag_packet_metadata() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, "---\n---\n").unwrap();
        let schedule = crate::auto_dag::build_schedule(
            &doc,
            &["do #prep".to_string(), "do #report after #prep".to_string()],
            None,
            crate::auto_dag::classify_session_review_log(""),
            "queue",
        )
        .unwrap();

        let index = create_for_schedule(
            &doc,
            &schedule,
            CreateOptions {
                operation_doc: true,
                audit: true,
                budget: 512,
            },
        )
        .unwrap();

        assert_eq!(
            index.auto_dag_schedule_id,
            Some(schedule.schedule_id.clone())
        );
        assert_eq!(index.jobs.len(), 2);
        assert_eq!(index.jobs[0].auto_dag_node_id.as_deref(), Some("#prep"));
        assert!(
            index.jobs[0]
                .replay_commands
                .iter()
                .any(|command| command.contains("--resume-schedule"))
        );
        let packet = std::fs::read_to_string(dir.path().join(&index.jobs[0].path)).unwrap();
        assert!(packet.contains("auto_dag_schedule_id"));
        assert!(packet.contains("## DAG Schedule"));
        assert!(packet.contains("agent-doc-worker-result-v1"));
        let operation_doc = index.operation_doc.as_ref().unwrap();
        let operation = std::fs::read_to_string(dir.path().join(operation_doc)).unwrap();
        assert!(operation.contains("# Auto-DAG Operation"));
        assert!(operation.contains("batch 1: #prep"));
    }
}
