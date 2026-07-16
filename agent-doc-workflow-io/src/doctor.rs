//! Workflow invariant doctor for agent-doc session documents.
//!
//! The doctor is a diagnostic surface. It gathers durable facts from the
//! current document, optional preflight/session-check JSON captures, cycle state,
//! ops logs, controller inspection, git state, and the Lazily current document, then
//! evaluates the workflow invariant catalog into typed outcomes.

use anyhow::{Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};

use agent_doc_workflow::doctor::{
    ActorDoctorFacts, CycleStateDoctorFacts, EditorDoctorFacts, GitDoctorFacts, OpsLogDoctorFacts,
    PreflightDoctorFacts, SessionCheckDoctorFacts, WorkflowDoctorFacts, WorkflowDoctorReport,
    evaluate_catalog, format_text_report, ops_log_facts_from_content,
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LiveSessionCheckFacts {
    pub ok: Option<bool>,
    pub status: Option<String>,
    pub message: Option<String>,
    pub warnings: Vec<String>,
}

pub trait WorkflowDoctorEffects {
    fn inspect_session_check(&mut self, file: &Path) -> Result<Option<LiveSessionCheckFacts>>;

    fn inspect_actor(&mut self, project_root: &Path, file: &Path) -> Result<ActorDoctorFacts>;

    fn lazily_current_diverges(
        &mut self,
        file: &Path,
        disk_content: &str,
        project_root: &Path,
    ) -> Result<Option<bool>>;
}

pub struct NoopWorkflowDoctorEffects;

impl WorkflowDoctorEffects for NoopWorkflowDoctorEffects {
    fn inspect_session_check(&mut self, _file: &Path) -> Result<Option<LiveSessionCheckFacts>> {
        Ok(None)
    }

    fn inspect_actor(&mut self, _project_root: &Path, _file: &Path) -> Result<ActorDoctorFacts> {
        Ok(ActorDoctorFacts::default())
    }

    fn lazily_current_diverges(
        &mut self,
        _file: &Path,
        _disk_content: &str,
        _project_root: &Path,
    ) -> Result<Option<bool>> {
        Ok(None)
    }
}

pub fn run(
    file: &Path,
    options: WorkflowDoctorOptions,
    effects: &mut impl WorkflowDoctorEffects,
) -> Result<()> {
    let report = diagnose(file, &options, effects)?;
    if options.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print!("{}", format_text_report(&report));
    }
    Ok(())
}

pub fn diagnose(
    file: &Path,
    options: &WorkflowDoctorOptions,
    effects: &mut impl WorkflowDoctorEffects,
) -> Result<WorkflowDoctorReport> {
    let catalog = workflow_invariant_catalog();
    let (facts, warnings) = gather_facts(file, options, effects)?;
    Ok(evaluate_catalog(file, catalog, facts, warnings))
}

fn gather_facts(
    file: &Path,
    options: &WorkflowDoctorOptions,
    effects: &mut impl WorkflowDoctorEffects,
) -> Result<(WorkflowDoctorFacts, Vec<String>)> {
    let mut warnings = Vec::new();
    let canonical = file
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", file.display()))?;
    let project_root = agent_doc_project_root_io::project_root_containing(&canonical);
    let document_hash = agent_doc_fs::document_state_hash(&canonical).ok();
    let preflight = read_preflight_facts(options.preflight_json.as_deref())?;
    let mut session_check = read_session_check_json_facts(options.session_check_json.as_deref())?;
    merge_live_session_check(effects, file, &mut session_check, &mut warnings);
    let cycle_state = read_cycle_state(file, &mut warnings);
    let ops_log = read_ops_log(project_root.as_deref(), options.ops_limit, &mut warnings);
    let actor = read_actor_facts(effects, file, project_root.as_deref(), &mut warnings);
    let git = read_git_facts(file, &mut warnings);
    let editor = read_editor_facts(effects, file, project_root.as_deref(), &mut warnings);
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
    effects: &mut impl WorkflowDoctorEffects,
    file: &Path,
    facts: &mut SessionCheckDoctorFacts,
    warnings: &mut Vec<String>,
) {
    match effects.inspect_session_check(file) {
        Ok(Some(live)) => {
            let interrupted =
                live.ok == Some(false) || live.status.as_deref() == Some("interrupted");
            if interrupted {
                facts.ok = live.ok.or(Some(false));
                facts.status = live.status.or_else(|| Some("interrupted".to_string()));
                facts.message = live.message;
            } else {
                if facts.ok.is_none() {
                    facts.ok = live.ok;
                }
                if facts.status.is_none() {
                    facts.status = live.status;
                }
                if facts.message.is_none() {
                    facts.message = live.message;
                }
            }
            facts.warnings.extend(live.warnings);
        }
        Ok(None) => {}
        Err(err) => warnings.push(format!("session-check inspection unavailable: {err}")),
    }
}

fn read_cycle_state(file: &Path, warnings: &mut Vec<String>) -> CycleStateDoctorFacts {
    match agent_doc_cycle_state_io::load_with_closeout_projection(file) {
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
    ops_log_facts_from_content(&content, limit)
}

fn read_actor_facts(
    effects: &mut impl WorkflowDoctorEffects,
    file: &Path,
    project_root: Option<&Path>,
    warnings: &mut Vec<String>,
) -> ActorDoctorFacts {
    let Some(root) = project_root else {
        return ActorDoctorFacts::default();
    };
    match effects.inspect_actor(root, file) {
        Ok(facts) => facts,
        Err(err) => {
            warnings.push(format!("controller actor inspection unavailable: {err}"));
            ActorDoctorFacts::default()
        }
    }
}

fn read_git_facts(file: &Path, warnings: &mut Vec<String>) -> GitDoctorFacts {
    let snapshot_status = match agent_doc_snapshot_io::verify_snapshot_committed(file) {
        Ok(status) => Some(format!("{status:?}")),
        Err(err) => {
            warnings.push(format!("snapshot git verification unavailable: {err}"));
            None
        }
    };
    let tracked_modified_paths = match agent_doc_git_io::status::tracked_modified_paths(file) {
        Ok(paths) => paths,
        Err(err) => {
            warnings.push(format!("tracked git status unavailable: {err}"));
            Vec::new()
        }
    };
    let drift = match agent_doc_git_io::submodule::submodule_pointer_drift(file) {
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
    effects: &mut impl WorkflowDoctorEffects,
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
            warnings.push(format!("Lazily current-document check unavailable: {err}"));
            String::new()
        }
    };
    let lazily_current_diverges = if disk.is_empty() {
        None
    } else {
        match effects.lazily_current_diverges(file, &disk, root) {
            Ok(value) => value,
            Err(err) => {
                warnings.push(format!("Lazily current-document check unavailable: {err}"));
                None
            }
        }
    };
    EditorDoctorFacts {
        patches_dir_present: root.join(".agent-doc/patches").is_dir(),
        legacy_ack_content_dir_present: root.join(".agent-doc/ack-content").is_dir(),
        legacy_live_buffer_dir_present: root.join(".agent-doc/live-buffer").is_dir(),
        lazily_current_diverges,
    }
}

fn read_json(path: &Path) -> Result<Value> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    Ok(serde_json::from_str(&content)?)
}
