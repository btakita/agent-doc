//! Binary-owned auto-DAG planning for queued `do #id` work.
//!
//! The scheduler is intentionally deterministic: it expands queue directives
//! into nodes, validates dependency edges, computes source-order antichain
//! batches, records graph/job replay metadata, and classifies session-review
//! log families before any child work is launched.

use agent_doc_work_graph::schedule::{
    AutoDagGraphEvidence, AutoDagGraphTargetEvidence, AutoDagNodeState, AutoDagSchedule,
    AutoDagScheduleBuildInput, AutoDagScheduleSeed, SCHEDULE_CONTRACT_VERSION,
    SessionReviewGuardReport, build_schedule as build_auto_dag_schedule,
    classify_session_review_log, schedule_seed, target_evidence_for_schedule,
    update_schedule_node_state, validate_schedule_graph_evidence,
};
use anyhow::{Context, Result, bail};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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

    let parent_doc = file.display().to_string();
    let seed = schedule_seed(parent_doc.clone(), tasks)?;
    let graph_evidence = schedule_graph_evidence_from_tsift(graph_evidence);
    let warnings = validate_schedule_graph_evidence(graph_evidence.as_ref(), &seed)?;
    let schedule_id = schedule_id(file, &seed)?;
    let graph_status = graph_evidence
        .as_ref()
        .map(|evidence| evidence.graph_status.clone())
        .unwrap_or_else(|| "missing".to_string());
    let replay_command = format!(
        "agent-doc orchestrate {} --mode dag --resume-schedule {}",
        file.display(),
        schedule_id
    );

    build_auto_dag_schedule(AutoDagScheduleBuildInput {
        schedule_id,
        parent_doc,
        source_kind: source_kind.to_string(),
        created_at_unix: unix_now(),
        graph_status,
        guard,
        tasks,
        warnings,
        replay_command,
        target_evidence: target_evidence_for_schedule(graph_evidence.as_ref()),
    })
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
    agent_doc_document_realtime_io::atomic_write_through_authority(
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
    update_schedule_node_state(&mut schedule, node_id, state)?;
    write_schedule(file, &schedule)?;
    Ok(())
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

fn schedule_graph_evidence_from_tsift(
    graph_evidence: Option<&crate::tsift_graph::TsiftGraphEvidencePlan>,
) -> Option<AutoDagGraphEvidence> {
    graph_evidence.map(|graph| AutoDagGraphEvidence {
        graph_status: graph.graph_db_status.status.clone(),
        parallel_dispatch_blocker: graph.conflict_matrix.parallel_dispatch_blocker(),
        targets: graph
            .prompt_target_handles
            .iter()
            .map(|handle| AutoDagGraphTargetEvidence {
                target: handle.target.clone(),
                replay_commands: handle.replay_commands.clone(),
                repair_commands: handle.repair_commands.clone(),
                ownership_packet_ids: ownership_packet_ids_for_target(graph, &handle.target),
            })
            .collect(),
    })
}

fn ownership_packet_ids_for_target(
    graph: &crate::tsift_graph::TsiftGraphEvidencePlan,
    target: &str,
) -> Vec<String> {
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
    packet_ids.into_iter().collect()
}

fn schedule_id(file: &Path, seed: &AutoDagScheduleSeed) -> Result<String> {
    let seed = serde_json::json!({
        "file": file.display().to_string(),
        "schedule": seed,
    });
    let doc_hash = agent_doc_fs::document_state_hash(file)?;
    let seed_hash = agent_doc_hash::content_hash(&seed.to_string());
    Ok(format!("dag-{}-{}", &doc_hash[..8], &seed_hash[..12]))
}

fn project_root(file: &Path) -> Result<PathBuf> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    agent_doc_fs::find_project_root(&canonical)
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
