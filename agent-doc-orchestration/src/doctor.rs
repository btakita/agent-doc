//! Workflow invariant doctor for agent-doc session documents.
//!
//! The doctor is a diagnostic surface. It gathers durable facts from the
//! current document, optional preflight/session-check JSON captures, cycle state,
//! ops logs, controller inspection, git state, and editor sidecars, then
//! evaluates the workflow invariant catalog into typed outcomes.

use anyhow::{Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};

use agent_doc_workflow::doctor::{
    ActorDoctorFacts, CycleStateDoctorFacts, EditorDoctorFacts, GitDoctorFacts, OpsLogDoctorFacts,
    PreflightDoctorFacts, SessionCheckDoctorFacts, WorkflowDoctorFacts, WorkflowDoctorReport,
    classify_ops_marker, evaluate_catalog,
};
use agent_doc_workflow::doctor_json::{project_preflight_facts, project_session_check_facts};
use agent_doc_workflow::invariants::workflow_invariant_catalog;

const DEFAULT_OPS_LIMIT: usize = 200;

#[derive(Debug, Clone)]
pub struct WorkflowDoctorOptions {
    pub preflight_json: Option<PathBuf>,
    pub session_check_json: Option<PathBuf>,
    pub ops_limit: usize,
    pub json: bool,
}

impl Default for WorkflowDoctorOptions {
    fn default() -> Self {
        Self {
            preflight_json: None,
            session_check_json: None,
            ops_limit: DEFAULT_OPS_LIMIT,
            json: false,
        }
    }
}

pub fn run(file: &Path, options: WorkflowDoctorOptions) -> Result<()> {
    let report = diagnose(file, &options)?;
    if options.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_text_report(&report);
    }
    Ok(())
}

pub fn diagnose(file: &Path, options: &WorkflowDoctorOptions) -> Result<WorkflowDoctorReport> {
    let catalog = workflow_invariant_catalog();
    let (facts, warnings) = gather_facts(file, options)?;
    Ok(evaluate_catalog(file, catalog, facts, warnings))
}

fn gather_facts(
    file: &Path,
    options: &WorkflowDoctorOptions,
) -> Result<(WorkflowDoctorFacts, Vec<String>)> {
    let mut warnings = Vec::new();
    let canonical = file
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", file.display()))?;
    let project_root = agent_doc_fs::find_project_root(&canonical);
    let document_hash = crate::snapshot::doc_hash(&canonical).ok();
    let preflight = read_preflight_facts(options.preflight_json.as_deref())?;
    let mut session_check = read_session_check_json_facts(options.session_check_json.as_deref())?;
    merge_live_session_check(file, &mut session_check, &mut warnings);
    let cycle_state = read_cycle_state(file, &mut warnings);
    let ops_log = read_ops_log(project_root.as_deref(), options.ops_limit, &mut warnings);
    let actor = read_actor_facts(file, project_root.as_deref(), &mut warnings);
    let git = read_git_facts(file, &mut warnings);
    let editor = read_editor_facts(file, project_root.as_deref(), &mut warnings);
    Ok((
        WorkflowDoctorFacts {
            project_root: project_root.map(|root| root.display().to_string()),
            document_hash,
            preflight,
            session_check,
            cycle_state,
            ops_log,
            actor,
            git,
            editor,
        },
        warnings,
    ))
}

fn read_preflight_facts(path: Option<&Path>) -> Result<PreflightDoctorFacts> {
    let Some(path) = path else {
        return Ok(PreflightDoctorFacts::default());
    };
    let value = read_json(path).with_context(|| {
        format!(
            "failed to read preflight JSON facts from {}",
            path.display()
        )
    })?;
    Ok(project_preflight_facts(&value))
}

fn read_session_check_json_facts(path: Option<&Path>) -> Result<SessionCheckDoctorFacts> {
    let Some(path) = path else {
        return Ok(SessionCheckDoctorFacts::default());
    };
    let value = read_json(path).with_context(|| {
        format!(
            "failed to read session-check JSON facts from {}",
            path.display()
        )
    })?;
    Ok(project_session_check_facts(&value))
}

fn merge_live_session_check(
    file: &Path,
    facts: &mut SessionCheckDoctorFacts,
    warnings: &mut Vec<String>,
) {
    match crate::session_check::inspect_with_warnings(file) {
        Ok(report) => {
            match report.status {
                crate::session_check::SessionCheckStatus::Ok(message) => {
                    if facts.ok.is_none() {
                        facts.ok = Some(true);
                    }
                    if facts.status.is_none() {
                        facts.status = Some("ok".to_string());
                    }
                    if facts.message.is_none() {
                        facts.message = Some(message);
                    }
                }
                crate::session_check::SessionCheckStatus::Interrupted(message) => {
                    facts.ok = Some(false);
                    facts.status = Some("interrupted".to_string());
                    facts.message = Some(message);
                }
            }
            facts.warnings.extend(report.warnings);
        }
        Err(err) => warnings.push(format!("session-check inspection unavailable: {err}")),
    }
}

fn read_cycle_state(file: &Path, warnings: &mut Vec<String>) -> CycleStateDoctorFacts {
    match crate::cycle_state::load(file) {
        Ok(Some(state)) => {
            let open = state.is_open();
            CycleStateDoctorFacts {
                present: true,
                cycle_id: Some(state.cycle_id),
                phase: Some(format!("{:?}", state.phase)),
                open: Some(open),
                last_event: Some(state.last_event),
                capture_id: state.capture_id,
                response_sha256: state.response_sha256,
                baseline_file: state.baseline_file,
                prompt_targets: state.prompt_targets,
                queue_task_id: state.queue_task_id,
                turn_id: state.turn_id,
                had_pending_mutations: state.had_pending_mutations,
                pending_done_ids: state.pending_done_ids,
                pending_gated_ids: state.pending_gated_ids,
            }
        }
        Ok(None) => CycleStateDoctorFacts::default(),
        Err(err) => {
            warnings.push(format!("cycle-state inspection unavailable: {err}"));
            CycleStateDoctorFacts::default()
        }
    }
}

fn read_ops_log(
    project_root: Option<&Path>,
    limit: usize,
    warnings: &mut Vec<String>,
) -> OpsLogDoctorFacts {
    let Some(root) = project_root else {
        return OpsLogDoctorFacts::default();
    };
    let path = root.join(".agent-doc/logs/ops.log");
    let content = match agent_doc_fs::read_optional_text(&path) {
        Ok(Some(content)) => content,
        Ok(None) => return OpsLogDoctorFacts::default(),
        Err(err) => {
            warnings.push(format!("ops.log inspection unavailable: {err}"));
            return OpsLogDoctorFacts::default();
        }
    };
    let lines: Vec<&str> = content.lines().rev().take(limit.max(1)).collect();
    let mut markers = Vec::new();
    for line in lines.iter().rev() {
        if let Some(marker) = classify_ops_marker(line) {
            markers.push(marker.to_string());
        }
    }
    markers.sort();
    markers.dedup();
    OpsLogDoctorFacts {
        present: true,
        scanned_lines: lines.len(),
        markers,
    }
}

fn read_actor_facts(
    file: &Path,
    project_root: Option<&Path>,
    warnings: &mut Vec<String>,
) -> ActorDoctorFacts {
    let Some(root) = project_root else {
        return ActorDoctorFacts::default();
    };
    let inspection = match crate::project_controller::inspect_actor(root, Some(file), None, None) {
        Ok(inspection) => inspection,
        Err(err) => {
            warnings.push(format!("controller actor inspection unavailable: {err}"));
            return ActorDoctorFacts::default();
        }
    };
    let mut facts = ActorDoctorFacts {
        inspection_available: true,
        state: None,
        generation: None,
        pane: None,
        supervisor_pid: None,
        controller_fresh: None,
        supervisor_fresh: None,
        guidance: None,
    };
    if let Some(record) = inspection.record {
        facts.state = Some(record.state.as_str().to_string());
        facts.generation = Some(record.generation);
        facts.pane = Some(record.pane_id);
    }
    if let Some(lease) = inspection.supervisor_lease {
        facts.supervisor_pid = lease.supervisor_pid;
    }
    if let Some(freshness) = inspection.freshness {
        facts.controller_fresh = freshness.controller.matches_installed;
        facts.supervisor_fresh = freshness
            .route_owned_supervisor
            .as_ref()
            .and_then(|process| process.matches_installed);
        facts.guidance = Some(freshness.guidance);
    }
    facts
}

fn read_git_facts(file: &Path, warnings: &mut Vec<String>) -> GitDoctorFacts {
    let snapshot_status = match crate::git::verify_snapshot_committed(file) {
        Ok(status) => Some(format!("{status:?}")),
        Err(err) => {
            warnings.push(format!("snapshot git verification unavailable: {err}"));
            None
        }
    };
    let tracked_modified_paths = match crate::git::tracked_modified_paths(file) {
        Ok(paths) => paths,
        Err(err) => {
            warnings.push(format!("tracked git status unavailable: {err}"));
            Vec::new()
        }
    };
    let drift = match crate::git::submodule_pointer_drift(file) {
        Ok(drift) => drift,
        Err(err) => {
            warnings.push(format!("parent gitlink inspection unavailable: {err}"));
            None
        }
    };
    let mut facts = GitDoctorFacts {
        snapshot_status,
        tracked_modified_paths,
        parent_gitlink_stale: false,
        parent_gitlink_path: None,
        parent_gitlink_parent_head: None,
        parent_gitlink_submodule_head: None,
    };
    if let Some(drift) = drift {
        facts.parent_gitlink_stale = true;
        facts.parent_gitlink_path = Some(drift.relative_path);
        facts.parent_gitlink_parent_head = drift.parent_head;
        facts.parent_gitlink_submodule_head = Some(drift.submodule_head);
    }
    facts
}

fn read_editor_facts(
    file: &Path,
    project_root: Option<&Path>,
    warnings: &mut Vec<String>,
) -> EditorDoctorFacts {
    let Some(root) = project_root else {
        return EditorDoctorFacts::default();
    };
    let disk = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(err) => {
            warnings.push(format!("editor live-buffer check unavailable: {err}"));
            String::new()
        }
    };
    let live_buffer_diverges = if disk.is_empty() {
        None
    } else {
        Some(crate::realtime_model::durable_buffer_state(file, &disk).is_some())
    };
    EditorDoctorFacts {
        patches_dir_present: root.join(".agent-doc/patches").is_dir(),
        ack_content_dir_present: root.join(".agent-doc/ack-content").is_dir(),
        live_buffer_dir_present: root.join(".agent-doc/live-buffer").is_dir(),
        live_buffer_diverges,
    }
}

fn read_json(path: &Path) -> Result<Value> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    Ok(serde_json::from_str(&content)?)
}

fn print_text_report(report: &WorkflowDoctorReport) {
    println!(
        "workflow doctor: {} outcome={}",
        report.file,
        report.outcome.as_str()
    );
    for warning in &report.warnings {
        println!("warning: {warning}");
    }
    for result in &report.results {
        println!(
            "- {} outcome={} title={}",
            result.id,
            result.outcome.as_str(),
            result.title
        );
        for evidence in &result.evidence {
            println!("  evidence: {evidence}");
        }
        for missing in &result.missing_fact_sources {
            println!("  missing: {missing}");
        }
        for marker in &result.disproof_markers {
            println!("  disproof: {marker}");
        }
        for command in &result.repair_commands {
            println!("  repair: {command}");
        }
        for action in &result.operator_actions {
            println!("  operator: {action}");
        }
    }
}
