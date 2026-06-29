//! Binary-owned auto-DAG planning for queued `do #id` work.
//!
//! The scheduler is intentionally deterministic: it expands queue directives
//! into nodes, validates dependency edges, computes source-order antichain
//! batches, records graph/job replay metadata, and classifies session-review
//! log families before any child work is launched.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const SCHEDULE_CONTRACT_VERSION: &str = "agent-doc-auto-dag-schedule-v1";

/// `agent-doc auto-dag <FILE>` entry point for first-class backlog/review graph
/// planning. Pure graph analysis/rendering lives in `agent-doc-work-graph`; the
/// binary owns file IO and terminal output.
pub(crate) fn run_command(file: &Path, json: bool) -> Result<()> {
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("auto-dag: read {}", file.display()))?;
    let dag = agent_doc_work_graph::analyze_document(&content)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&dag)?);
    } else {
        println!("# Auto-DAG: completion work-graph for {}\n", file.display());
        println!("{}", agent_doc_work_graph::render_mermaid(&dag));
        println!("## Completion order\n");
        print!("{}", agent_doc_work_graph::render_nested_list(&dag));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AutoDagSchedule {
    pub(crate) contract_version: String,
    pub(crate) schedule_id: String,
    pub(crate) parent_doc: String,
    pub(crate) source_kind: String,
    pub(crate) created_at_unix: u64,
    pub(crate) graph_status: String,
    pub(crate) guard: SessionReviewGuardReport,
    pub(crate) nodes: Vec<AutoDagNode>,
    pub(crate) batches: Vec<Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AutoDagNode {
    pub(crate) id: String,
    pub(crate) target: Option<String>,
    pub(crate) label: String,
    pub(crate) prompt: String,
    pub(crate) deps: Vec<String>,
    pub(crate) state: AutoDagNodeState,
    pub(crate) attempt_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) replay_commands: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) repair_commands: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AutoDagNodeState {
    Pending,
    Ready,
    Running,
    Complete,
    Blocked,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SessionReviewGuardReport {
    pub(crate) action: SessionReviewGuardAction,
    pub(crate) scanned_lines: usize,
    pub(crate) families: Vec<SessionReviewFamilyReport>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) recommendations: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SessionReviewGuardAction {
    Proceed,
    CompactFirst,
    RestartFirst,
    FixtureFixFirst,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SessionReviewFamilyReport {
    pub(crate) family: SessionReviewFamily,
    pub(crate) count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) samples: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SessionReviewFamily {
    PromptBudget,
    CacheResend,
    RestartLoop,
    NoopCloseout,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedTask {
    id: String,
    target: Option<String>,
    label: String,
    prompt: String,
    deps: Vec<String>,
}

#[derive(Debug, Default)]
struct Metadata {
    id: Option<String>,
    deps: Vec<String>,
}

pub(crate) fn build_schedule(
    file: &Path,
    tasks: &[String],
    graph_evidence: Option<&crate::tsift_graph::TsiftGraphEvidencePlan>,
    guard: SessionReviewGuardReport,
    source_kind: &str,
) -> Result<AutoDagSchedule> {
    if tasks.is_empty() {
        bail!("auto-DAG requires at least one task");
    }

    let parsed = parse_tasks(tasks)?;
    let batches = antichain_batches(&parsed)?;
    let warnings = validate_graph_evidence(graph_evidence, &parsed, &batches)?;
    let schedule_id = schedule_id(file, &parsed, &batches)?;
    let parent_doc = file.display().to_string();
    let graph_status = graph_evidence
        .map(|evidence| evidence.graph_db_status.status.clone())
        .unwrap_or_else(|| "missing".to_string());
    let replay_command = format!(
        "agent-doc orchestrate {} --mode dag --resume-schedule {}",
        file.display(),
        schedule_id
    );

    let nodes = parsed
        .into_iter()
        .map(|task| {
            let mut replay_commands = vec![replay_command.clone()];
            let mut repair_commands = Vec::new();
            if let (Some(graph), Some(target)) = (graph_evidence, task.target.as_deref())
                && let Some(handle) = graph
                    .prompt_target_handles
                    .iter()
                    .find(|handle| handle.target.eq_ignore_ascii_case(target))
            {
                replay_commands.extend(handle.replay_commands.clone());
                repair_commands.extend(handle.repair_commands.clone());
            }
            AutoDagNode {
                id: task.id,
                target: task.target,
                label: task.label,
                prompt: task.prompt,
                deps: task.deps,
                state: AutoDagNodeState::Pending,
                attempt_count: 0,
                replay_commands,
                repair_commands,
            }
        })
        .collect();

    let mut schedule = AutoDagSchedule {
        contract_version: SCHEDULE_CONTRACT_VERSION.to_string(),
        schedule_id,
        parent_doc,
        source_kind: source_kind.to_string(),
        created_at_unix: unix_now(),
        graph_status,
        guard,
        nodes,
        batches,
        warnings,
    };
    mark_ready_nodes(&mut schedule);
    Ok(schedule)
}

pub(crate) fn schedule_path(file: &Path, schedule_id: &str) -> Result<PathBuf> {
    let root = project_root(file)?;
    Ok(root
        .join(".agent-doc")
        .join("schedules")
        .join(format!("{schedule_id}.json")))
}

pub(crate) fn write_schedule(file: &Path, schedule: &AutoDagSchedule) -> Result<PathBuf> {
    let path = schedule_path(file, &schedule.schedule_id)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    agent_doc_orchestration::write::atomic_write_pub(
        &path,
        &serde_json::to_string_pretty(schedule).context("failed to serialize auto-DAG schedule")?,
    )?;
    Ok(path)
}

pub(crate) fn load_schedule(file: &Path, schedule_id: &str) -> Result<AutoDagSchedule> {
    let path = schedule_path(file, schedule_id)?;
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let schedule: AutoDagSchedule = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    if schedule.contract_version != SCHEDULE_CONTRACT_VERSION {
        bail!(
            "schedule {} has unsupported contract_version `{}`",
            schedule_id,
            schedule.contract_version
        );
    }
    Ok(schedule)
}

pub(crate) fn update_node_state(
    file: &Path,
    schedule_id: &str,
    node_id: &str,
    state: AutoDagNodeState,
) -> Result<()> {
    let mut schedule = load_schedule(file, schedule_id)?;
    let Some(node) = schedule.nodes.iter_mut().find(|node| node.id == node_id) else {
        bail!("schedule `{schedule_id}` has no node `{node_id}`");
    };
    if state == AutoDagNodeState::Running {
        node.attempt_count += 1;
    }
    node.state = state;
    mark_ready_nodes(&mut schedule);
    write_schedule(file, &schedule)?;
    Ok(())
}

pub(crate) fn guard_blocker(schedule: &AutoDagSchedule) -> Option<String> {
    match schedule.guard.action {
        SessionReviewGuardAction::Proceed => None,
        SessionReviewGuardAction::CompactFirst => Some(format!(
            "auto-DAG schedule {} gated by prompt-budget/cache-resend history; compact the exchange before dispatch",
            schedule.schedule_id
        )),
        SessionReviewGuardAction::RestartFirst => Some(format!(
            "auto-DAG schedule {} gated by restart-loop history; restart or repair the harness before dispatch",
            schedule.schedule_id
        )),
        SessionReviewGuardAction::FixtureFixFirst => Some(format!(
            "auto-DAG schedule {} gated by repeated noop-closeout history; fix the deterministic fixture/closeout path before dispatch",
            schedule.schedule_id
        )),
    }
}

pub(crate) fn session_review_guard_for_file(file: &Path) -> Result<SessionReviewGuardReport> {
    let root = project_root(file)?;
    let log_path = root.join(".agent-doc/logs/ops.log");
    let contents = match std::fs::read_to_string(&log_path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => {
            return Err(err).with_context(|| format!("failed to read {}", log_path.display()));
        }
    };
    Ok(classify_session_review_log(&contents))
}

pub(crate) fn classify_session_review_log(contents: &str) -> SessionReviewGuardReport {
    const MAX_SAMPLES: usize = 3;
    let mut counts: BTreeMap<SessionReviewFamily, (usize, Vec<String>)> = BTreeMap::new();
    let lines = contents.lines().rev().take(500).collect::<Vec<_>>();
    for line in lines.iter().rev() {
        let lower = line.to_ascii_lowercase();
        for family in families_for_line(&lower) {
            let entry = counts.entry(family).or_insert_with(|| (0, Vec::new()));
            entry.0 += 1;
            if entry.1.len() == MAX_SAMPLES {
                entry.1.remove(0);
            }
            entry.1.push(line.trim().to_string());
        }
    }

    let families = counts
        .into_iter()
        .map(|(family, (count, samples))| SessionReviewFamilyReport {
            family,
            count,
            samples,
        })
        .collect::<Vec<_>>();
    let count = |family| {
        families
            .iter()
            .find(|report| report.family == family)
            .map(|report| report.count)
            .unwrap_or(0)
    };
    let mut recommendations = Vec::new();
    let action = if count(SessionReviewFamily::RestartLoop) >= 2 {
        recommendations.push("restart or repair the managed harness before scheduling".to_string());
        SessionReviewGuardAction::RestartFirst
    } else if count(SessionReviewFamily::PromptBudget) > 0
        || count(SessionReviewFamily::CacheResend) > 0
    {
        recommendations.push("compact/reduce exchange context before scheduling".to_string());
        SessionReviewGuardAction::CompactFirst
    } else if count(SessionReviewFamily::NoopCloseout) >= 3 {
        recommendations
            .push("fix the repeated noop closeout fixture before scheduling".to_string());
        SessionReviewGuardAction::FixtureFixFirst
    } else {
        SessionReviewGuardAction::Proceed
    };

    SessionReviewGuardReport {
        action,
        scanned_lines: lines.len(),
        families,
        recommendations,
    }
}

fn families_for_line(line: &str) -> Vec<SessionReviewFamily> {
    let mut families = Vec::new();
    if line.contains("prompt_budget")
        || line.contains("prompt budget")
        || line.contains("context budget")
        || line.contains("prompt too large")
        || line.contains("maximum context")
        || line.contains("token budget exceeded")
    {
        families.push(SessionReviewFamily::PromptBudget);
    }
    if line.contains("cache_resend")
        || line.contains("cache resend")
        || line.contains("cache-resend")
        || line.contains("resend full context")
    {
        families.push(SessionReviewFamily::CacheResend);
    }
    if line.contains("restart_loop")
        || line.contains("restart loop")
        || line.contains("startup_miss")
        || line.contains("fresh_restart_retry")
    {
        families.push(SessionReviewFamily::RestartLoop);
    }
    if line.contains("noop_closeout")
        || line.contains("noop closeout")
        || line.contains("commit_noop")
        || line.contains("already_current")
    {
        families.push(SessionReviewFamily::NoopCloseout);
    }
    families
}

fn parse_tasks(tasks: &[String]) -> Result<Vec<ParsedTask>> {
    let mut parsed = Vec::new();
    for (idx, task) in tasks.iter().enumerate() {
        parsed.extend(parse_task(task, idx)?);
    }
    canonicalize_dependency_ids(&mut parsed)?;
    Ok(parsed)
}

fn parse_task(task: &str, index: usize) -> Result<Vec<ParsedTask>> {
    let normalized = normalize_task(task);
    if normalized.is_empty() {
        bail!("auto-DAG task {} is empty", index + 1);
    }
    let (metadata, prompt) = split_metadata(&normalized)?;
    if prompt.is_empty() {
        bail!("auto-DAG task {} is missing a prompt", index + 1);
    }

    let mut deps = metadata.deps;
    deps.extend(extract_natural_dependencies(&prompt));
    dedup(&mut deps);

    let do_targets = extract_schedule_do_targets(&prompt);
    if do_targets.len() > 1 {
        if metadata.id.is_some() {
            bail!("compound auto-DAG `do` directives cannot also use a single explicit id");
        }
        return Ok(do_targets
            .into_iter()
            .map(|target| {
                let id = format!("#{target}");
                ParsedTask {
                    id,
                    target: Some(target.clone()),
                    label: format!("do #{target}"),
                    prompt: format!("do #{target}. From compound directive: {prompt}"),
                    deps: deps.clone(),
                }
            })
            .collect());
    }

    let target = do_targets.first().cloned();
    let id = metadata
        .id
        .or_else(|| target.as_ref().map(|target| format!("#{target}")))
        .or_else(|| extract_first_hash_id(&prompt))
        .unwrap_or_else(|| format!("step-{}", index + 1));
    Ok(vec![ParsedTask {
        id,
        target,
        label: prompt.clone(),
        prompt,
        deps,
    }])
}

fn split_metadata(task: &str) -> Result<(Metadata, String)> {
    let trimmed = task.trim();
    let Some(rest) = trimmed.strip_prefix('[') else {
        return Ok((Metadata::default(), trimmed.to_string()));
    };
    let closing = rest
        .find(']')
        .ok_or_else(|| anyhow::anyhow!("auto-DAG task metadata is missing closing `]`"))?;
    let metadata_text = &rest[..closing];
    let prompt = rest[closing + 1..].trim().to_string();
    let metadata = parse_metadata(metadata_text)?;
    Ok((metadata, prompt))
}

fn parse_metadata(metadata: &str) -> Result<Metadata> {
    let mut parsed = Metadata::default();
    for token in metadata.split_whitespace() {
        if let Some(value) = token
            .strip_prefix("after=")
            .or_else(|| token.strip_prefix("deps="))
        {
            parsed.deps.extend(parse_dependency_list(value));
            continue;
        }
        if let Some(value) = token.strip_prefix("id=") {
            let value = value.trim();
            if value.is_empty() {
                bail!("auto-DAG task metadata has empty `id=`");
            }
            parsed.id = Some(canonical_id(value));
            continue;
        }
        if parsed.id.is_none() {
            parsed.id = Some(canonical_id(token));
            continue;
        }
        bail!("unsupported auto-DAG task metadata token `{token}`");
    }
    dedup(&mut parsed.deps);
    Ok(parsed)
}

fn parse_dependency_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|dep| !dep.is_empty())
        .map(canonical_id)
        .collect()
}

fn extract_natural_dependencies(prompt: &str) -> Vec<String> {
    let lower = prompt.to_ascii_lowercase();
    let phrases = ["depends on", "blocked by", "requires", "after"];
    let mut deps = Vec::new();
    for phrase in phrases {
        let mut start = 0usize;
        while let Some(pos) = lower[start..].find(phrase) {
            let absolute = start + pos + phrase.len();
            let tail = &prompt[absolute..];
            let end = tail.find(['.', ';', '\n']).unwrap_or(tail.len());
            deps.extend(
                extract_hash_ids(&tail[..end])
                    .into_iter()
                    .map(|id| format!("#{id}")),
            );
            start = absolute;
        }
    }
    dedup(&mut deps);
    deps
}

fn canonicalize_dependency_ids(tasks: &mut [ParsedTask]) -> Result<()> {
    let ids = tasks
        .iter()
        .map(|task| task.id.clone())
        .collect::<HashSet<_>>();
    let mut normalized_map = BTreeMap::new();
    for id in &ids {
        normalized_map.insert(strip_hash(id).to_ascii_lowercase(), id.clone());
    }
    for task in tasks {
        for dep in &mut task.deps {
            if ids.contains(dep) {
                continue;
            }
            if let Some(mapped) = normalized_map.get(&strip_hash(dep).to_ascii_lowercase()) {
                *dep = mapped.clone();
            }
        }
    }
    Ok(())
}

fn antichain_batches(tasks: &[ParsedTask]) -> Result<Vec<Vec<String>>> {
    validate_unique_and_known_deps(tasks)?;
    let mut completed = HashSet::new();
    let mut remaining = (0..tasks.len()).collect::<Vec<_>>();
    let mut batches = Vec::new();

    while !remaining.is_empty() {
        let ready = remaining
            .iter()
            .copied()
            .filter(|idx| tasks[*idx].deps.iter().all(|dep| completed.contains(dep)))
            .collect::<Vec<_>>();
        if ready.is_empty() {
            let blocked = remaining
                .iter()
                .map(|idx| tasks[*idx].id.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            bail!("auto-DAG dependency cycle detected among: {blocked}");
        }
        let batch_ids = ready
            .iter()
            .map(|idx| tasks[*idx].id.clone())
            .collect::<Vec<_>>();
        for idx in ready {
            completed.insert(tasks[idx].id.clone());
            if let Some(pos) = remaining
                .iter()
                .position(|remaining_idx| *remaining_idx == idx)
            {
                remaining.remove(pos);
            }
        }
        batches.push(batch_ids);
    }

    Ok(batches)
}

fn validate_unique_and_known_deps(tasks: &[ParsedTask]) -> Result<()> {
    let mut ids = HashSet::new();
    for task in tasks {
        if !ids.insert(task.id.clone()) {
            bail!("duplicate auto-DAG task id `{}`", task.id);
        }
    }
    for task in tasks {
        for dep in &task.deps {
            if !ids.contains(dep) {
                bail!(
                    "auto-DAG task `{}` depends on unknown task `{}`",
                    task.id,
                    dep
                );
            }
        }
    }
    Ok(())
}

fn validate_graph_evidence(
    graph_evidence: Option<&crate::tsift_graph::TsiftGraphEvidencePlan>,
    tasks: &[ParsedTask],
    batches: &[Vec<String>],
) -> Result<Vec<String>> {
    let Some(graph) = graph_evidence else {
        return Ok(vec![
            "graph_evidence=missing; schedule requires parent review".to_string(),
        ]);
    };
    if !matches!(graph.graph_db_status.status.as_str(), "current" | "fresh") {
        bail!(
            "stale graph evidence: graph_db_status.status={}",
            graph.graph_db_status.status
        );
    }
    if batches.iter().any(|batch| batch.len() > 1)
        && let Some(blocker) = graph.conflict_matrix.parallel_dispatch_blocker()
    {
        bail!("graph conflict evidence blocked antichain dispatch: {blocker}");
    }

    for task in tasks.iter().filter(|task| task.target.is_some()) {
        let target = task.target.as_deref().unwrap();
        let matching_handles = graph
            .prompt_target_handles
            .iter()
            .filter(|handle| handle.target.eq_ignore_ascii_case(target))
            .count();
        if matching_handles == 0 {
            bail!("missing graph evidence for auto-DAG target #{target}");
        }
        if matching_handles > 1 {
            bail!("ambiguous graph evidence for auto-DAG target #{target}");
        }
        let mut packet_ids = BTreeSet::new();
        for packet in graph
            .conflict_matrix
            .worker_prompt_packets
            .iter()
            .filter(|packet| packet.target.eq_ignore_ascii_case(target))
        {
            packet_ids.insert(
                packet
                    .packet_id
                    .clone()
                    .unwrap_or_else(|| format!("matrix:{target}:{}", packet.rank)),
            );
        }
        if let Some(trace) = &graph.dispatch_trace {
            for packet in trace
                .worker_prompt_packets
                .iter()
                .filter(|packet| packet.target.eq_ignore_ascii_case(target))
            {
                packet_ids.insert(
                    packet
                        .packet_id
                        .clone()
                        .unwrap_or_else(|| format!("trace:{target}:{}", packet.rank)),
                );
            }
        }
        if packet_ids.is_empty() {
            bail!("missing worker ownership packet for auto-DAG target #{target}");
        }
        if packet_ids.len() > 1 {
            bail!("ambiguous worker ownership packets for auto-DAG target #{target}");
        }
    }
    Ok(Vec::new())
}

fn mark_ready_nodes(schedule: &mut AutoDagSchedule) {
    let complete = schedule
        .nodes
        .iter()
        .filter(|node| node.state == AutoDagNodeState::Complete)
        .map(|node| node.id.clone())
        .collect::<HashSet<_>>();
    for node in &mut schedule.nodes {
        if matches!(
            node.state,
            AutoDagNodeState::Complete
                | AutoDagNodeState::Running
                | AutoDagNodeState::Blocked
                | AutoDagNodeState::Failed
        ) {
            continue;
        }
        node.state = if node.deps.iter().all(|dep| complete.contains(dep)) {
            AutoDagNodeState::Ready
        } else {
            AutoDagNodeState::Pending
        };
    }
}

fn schedule_id(file: &Path, tasks: &[ParsedTask], batches: &[Vec<String>]) -> Result<String> {
    let seed = serde_json::json!({
        "file": file.display().to_string(),
        "tasks": tasks.iter().map(|task| serde_json::json!({
            "id": task.id,
            "target": task.target,
            "label": task.label,
            "prompt": task.prompt,
            "deps": task.deps,
        })).collect::<Vec<_>>(),
        "batches": batches,
    });
    let doc_hash = agent_doc_orchestration::snapshot::doc_hash(file)?;
    let seed_hash = agent_doc_orchestration::ops_log::content_hash(&seed.to_string());
    Ok(format!("dag-{}-{}", &doc_hash[..8], &seed_hash[..12]))
}

fn extract_first_hash_id(text: &str) -> Option<String> {
    extract_hash_ids(text)
        .into_iter()
        .next()
        .map(|id| format!("#{id}"))
}

fn extract_schedule_do_targets(prompt: &str) -> Vec<String> {
    let normalized = prompt.trim().trim_start_matches('❯').trim();
    let lower = normalized.to_ascii_lowercase();
    let Some(rest_lower) = lower.strip_prefix("do ") else {
        return Vec::new();
    };
    let rest_start = normalized.len() - rest_lower.len();
    let rest = &normalized[rest_start..];
    let cut = [" after ", " depends on ", " blocked by ", " requires "]
        .iter()
        .filter_map(|phrase| rest.to_ascii_lowercase().find(phrase))
        .min()
        .unwrap_or(rest.len());
    extract_hash_ids(&rest[..cut])
}

fn extract_hash_ids(text: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let chars = text.chars().collect::<Vec<_>>();
    let mut idx = 0usize;
    while idx < chars.len() {
        if chars[idx] != '#' {
            idx += 1;
            continue;
        }
        idx += 1;
        let start = idx;
        while idx < chars.len()
            && (chars[idx].is_ascii_alphanumeric() || matches!(chars[idx], '_' | '-'))
        {
            idx += 1;
        }
        if start == idx {
            continue;
        }
        let id = chars[start..idx].iter().collect::<String>();
        if !ids.iter().any(|existing| existing == &id) {
            ids.push(id);
        }
    }
    ids
}

fn canonical_id(value: &str) -> String {
    let trimmed = value.trim().trim_matches(|ch| matches!(ch, '[' | ']'));
    if let Some(rest) = trimmed.strip_prefix('#') {
        format!("#{rest}")
    } else {
        trimmed.to_string()
    }
}

fn strip_hash(value: &str) -> &str {
    value.strip_prefix('#').unwrap_or(value)
}

fn normalize_task(task: &str) -> String {
    task.trim().trim_start_matches('❯').trim().to_string()
}

fn dedup(items: &mut Vec<String>) {
    let mut seen = HashSet::new();
    items.retain(|item| seen.insert(item.to_ascii_lowercase()));
}

fn project_root(file: &Path) -> Result<PathBuf> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    agent_doc_orchestration::snapshot::find_project_root(&canonical)
        .with_context(|| format!("could not find .agent-doc root for {}", file.display()))
}

fn unix_now() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs(),
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn schedule_expands_compound_do_directives() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "").unwrap();
        let guard = classify_session_review_log("");

        let schedule = build_schedule(
            &doc,
            &["do [#one] [#two]. update spec and tests".to_string()],
            None,
            guard,
            "queue",
        )
        .unwrap();

        assert_eq!(
            schedule
                .nodes
                .iter()
                .map(|node| node.id.as_str())
                .collect::<Vec<_>>(),
            vec!["#one", "#two"]
        );
        assert_eq!(
            schedule.batches,
            vec![vec!["#one".to_string(), "#two".to_string()]]
        );
        assert!(
            schedule.nodes[0]
                .prompt
                .contains("From compound directive: do [#one] [#two]")
        );
    }

    #[test]
    fn schedule_parses_natural_dependencies_into_antichains() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "").unwrap();

        let schedule = build_schedule(
            &doc,
            &[
                "do #prep".to_string(),
                "do #bench after #prep".to_string(),
                "do #report depends on #prep, #bench".to_string(),
            ],
            None,
            classify_session_review_log(""),
            "queue",
        )
        .unwrap();

        assert_eq!(
            schedule.batches,
            vec![
                vec!["#prep".to_string()],
                vec!["#bench".to_string()],
                vec!["#report".to_string()]
            ]
        );
    }

    #[test]
    fn schedule_rejects_unknown_deps_and_cycles() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "").unwrap();

        let err = build_schedule(
            &doc,
            &["do #a after #missing".to_string()],
            None,
            classify_session_review_log(""),
            "queue",
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("unknown task `#missing`"));

        let err = build_schedule(
            &doc,
            &["do #a after #b".to_string(), "do #b after #a".to_string()],
            None,
            classify_session_review_log(""),
            "queue",
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("dependency cycle"));
    }

    #[test]
    fn session_review_guard_classifies_log_families() {
        let report = classify_session_review_log(
            "prompt_budget exceeded\nstartup_miss pane=%1\nstartup_miss pane=%2\n",
        );
        assert_eq!(report.action, SessionReviewGuardAction::RestartFirst);
        assert!(
            report
                .families
                .iter()
                .any(|family| family.family == SessionReviewFamily::PromptBudget)
        );
        assert!(
            report.families.iter().any(
                |family| family.family == SessionReviewFamily::RestartLoop && family.count == 2
            )
        );
    }

    #[test]
    fn schedule_state_updates_are_persisted() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "").unwrap();
        let schedule = build_schedule(
            &doc,
            &["do #a".to_string()],
            None,
            classify_session_review_log(""),
            "queue",
        )
        .unwrap();
        write_schedule(&doc, &schedule).unwrap();

        update_node_state(&doc, &schedule.schedule_id, "#a", AutoDagNodeState::Running).unwrap();
        update_node_state(
            &doc,
            &schedule.schedule_id,
            "#a",
            AutoDagNodeState::Complete,
        )
        .unwrap();
        let loaded = load_schedule(&doc, &schedule.schedule_id).unwrap();
        assert_eq!(loaded.nodes[0].state, AutoDagNodeState::Complete);
        assert_eq!(loaded.nodes[0].attempt_count, 1);
    }
}
