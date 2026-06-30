use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

pub const SCHEDULE_CONTRACT_VERSION: &str = "agent-doc-auto-dag-schedule-v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoDagSchedule {
    pub contract_version: String,
    pub schedule_id: String,
    pub parent_doc: String,
    pub source_kind: String,
    pub created_at_unix: u64,
    pub graph_status: String,
    pub guard: SessionReviewGuardReport,
    pub nodes: Vec<AutoDagNode>,
    pub batches: Vec<Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoDagNode {
    pub id: String,
    pub target: Option<String>,
    pub label: String,
    pub prompt: String,
    pub deps: Vec<String>,
    pub state: AutoDagNodeState,
    pub attempt_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub replay_commands: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repair_commands: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoDagNodeState {
    Pending,
    Ready,
    Running,
    Complete,
    Blocked,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionReviewGuardReport {
    pub action: SessionReviewGuardAction,
    pub scanned_lines: usize,
    pub families: Vec<SessionReviewFamilyReport>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionReviewGuardAction {
    Proceed,
    CompactFirst,
    RestartFirst,
    FixtureFixFirst,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionReviewFamilyReport {
    pub family: SessionReviewFamily,
    pub count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub samples: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionReviewFamily {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AutoDagScheduleSeed {
    pub parent_doc: String,
    pub tasks: Vec<AutoDagScheduleSeedTask>,
    pub batches: Vec<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AutoDagScheduleSeedTask {
    pub id: String,
    pub target: Option<String>,
    pub label: String,
    pub prompt: String,
    pub deps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoDagTargetEvidence {
    pub target: String,
    pub replay_commands: Vec<String>,
    pub repair_commands: Vec<String>,
}

pub struct AutoDagScheduleBuildInput<'a> {
    pub schedule_id: String,
    pub parent_doc: String,
    pub source_kind: String,
    pub created_at_unix: u64,
    pub graph_status: String,
    pub guard: SessionReviewGuardReport,
    pub tasks: &'a [String],
    pub warnings: Vec<String>,
    pub replay_command: String,
    pub target_evidence: Vec<AutoDagTargetEvidence>,
}

pub fn schedule_seed(
    parent_doc: impl Into<String>,
    tasks: &[String],
) -> Result<AutoDagScheduleSeed> {
    let parsed = parse_tasks(tasks)?;
    let batches = antichain_batches(&parsed)?;
    let tasks = parsed
        .into_iter()
        .map(|task| AutoDagScheduleSeedTask {
            id: task.id,
            target: task.target,
            label: task.label,
            prompt: task.prompt,
            deps: task.deps,
        })
        .collect();
    Ok(AutoDagScheduleSeed {
        parent_doc: parent_doc.into(),
        tasks,
        batches,
    })
}

pub fn build_schedule(input: AutoDagScheduleBuildInput<'_>) -> Result<AutoDagSchedule> {
    if input.tasks.is_empty() {
        bail!("auto-DAG requires at least one task");
    }

    let parsed = parse_tasks(input.tasks)?;
    let batches = antichain_batches(&parsed)?;
    let nodes = parsed
        .into_iter()
        .map(|task| {
            let mut replay_commands = vec![input.replay_command.clone()];
            let mut repair_commands = Vec::new();
            if let Some(target) = task.target.as_deref()
                && let Some(evidence) = input
                    .target_evidence
                    .iter()
                    .find(|evidence| evidence.target.eq_ignore_ascii_case(target))
            {
                replay_commands.extend(evidence.replay_commands.clone());
                repair_commands.extend(evidence.repair_commands.clone());
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
        schedule_id: input.schedule_id,
        parent_doc: input.parent_doc,
        source_kind: input.source_kind,
        created_at_unix: input.created_at_unix,
        graph_status: input.graph_status,
        guard: input.guard,
        nodes,
        batches,
        warnings: input.warnings,
    };
    mark_ready_nodes(&mut schedule);
    Ok(schedule)
}

pub fn update_schedule_node_state(
    schedule: &mut AutoDagSchedule,
    node_id: &str,
    state: AutoDagNodeState,
) -> Result<()> {
    let Some(node) = schedule.nodes.iter_mut().find(|node| node.id == node_id) else {
        bail!(
            "schedule `{}` has no node `{}`",
            schedule.schedule_id,
            node_id
        );
    };
    if state == AutoDagNodeState::Running {
        node.attempt_count += 1;
    }
    node.state = state;
    mark_ready_nodes(schedule);
    Ok(())
}

pub fn guard_blocker(schedule: &AutoDagSchedule) -> Option<String> {
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

pub fn classify_session_review_log(contents: &str) -> SessionReviewGuardReport {
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
    canonicalize_dependency_ids(&mut parsed);
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

fn canonicalize_dependency_ids(tasks: &mut [ParsedTask]) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn build_test_schedule(tasks: &[String]) -> AutoDagSchedule {
        build_schedule(AutoDagScheduleBuildInput {
            schedule_id: "dag-test".to_string(),
            parent_doc: "session.md".to_string(),
            source_kind: "queue".to_string(),
            created_at_unix: 1,
            graph_status: "missing".to_string(),
            guard: classify_session_review_log(""),
            tasks,
            warnings: Vec::new(),
            replay_command: "agent-doc orchestrate session.md --mode dag --resume-schedule dag-test"
                .to_string(),
            target_evidence: Vec::new(),
        })
        .unwrap()
    }

    #[test]
    fn schedule_expands_compound_do_directives() {
        let schedule =
            build_test_schedule(&["do [#one] [#two]. update spec and tests".to_string()]);

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
        let schedule = build_test_schedule(&[
            "do #prep".to_string(),
            "do #bench after #prep".to_string(),
            "do #report depends on #prep, #bench".to_string(),
        ]);

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
        let err =
            build_schedule(AutoDagScheduleBuildInput {
                schedule_id: "dag-test".to_string(),
                parent_doc: "session.md".to_string(),
                source_kind: "queue".to_string(),
                created_at_unix: 1,
                graph_status: "missing".to_string(),
                guard: classify_session_review_log(""),
                tasks: &["do #a after #missing".to_string()],
                warnings: Vec::new(),
                replay_command:
                    "agent-doc orchestrate session.md --mode dag --resume-schedule dag-test"
                        .to_string(),
                target_evidence: Vec::new(),
            })
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown task `#missing`"));

        let err =
            build_schedule(AutoDagScheduleBuildInput {
                schedule_id: "dag-test".to_string(),
                parent_doc: "session.md".to_string(),
                source_kind: "queue".to_string(),
                created_at_unix: 1,
                graph_status: "missing".to_string(),
                guard: classify_session_review_log(""),
                tasks: &["do #a after #b".to_string(), "do #b after #a".to_string()],
                warnings: Vec::new(),
                replay_command:
                    "agent-doc orchestrate session.md --mode dag --resume-schedule dag-test"
                        .to_string(),
                target_evidence: Vec::new(),
            })
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
    fn schedule_state_transition_updates_attempts_and_readiness() {
        let mut schedule =
            build_test_schedule(&["do #a".to_string(), "do #b after #a".to_string()]);

        update_schedule_node_state(&mut schedule, "#a", AutoDagNodeState::Running).unwrap();
        update_schedule_node_state(&mut schedule, "#a", AutoDagNodeState::Complete).unwrap();

        assert_eq!(schedule.nodes[0].state, AutoDagNodeState::Complete);
        assert_eq!(schedule.nodes[0].attempt_count, 1);
        assert_eq!(schedule.nodes[1].state, AutoDagNodeState::Ready);
    }

    #[test]
    fn schedule_seed_uses_source_agnostic_parsed_tasks() {
        let seed = schedule_seed(
            "session.md",
            &["do #a".to_string(), "do #b after #a".to_string()],
        )
        .unwrap();

        assert_eq!(seed.parent_doc, "session.md");
        assert_eq!(seed.tasks[0].id, "#a");
        assert_eq!(
            seed.batches,
            vec![vec!["#a".to_string()], vec!["#b".to_string()]]
        );
    }
}
