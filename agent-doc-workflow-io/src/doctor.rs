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
    ActorDoctorFacts, CycleStateDoctorFacts, DocumentDoctorFacts, EditorDoctorFacts,
    GitDoctorFacts, OpsLogDoctorFacts, PreflightDoctorFacts, SessionCheckDoctorFacts,
    WorkflowDoctorFacts, WorkflowDoctorOutcome, WorkflowDoctorReport,
    document_doctor_facts_from_content, evaluate_catalog, format_text_report,
    ops_log_facts_from_content,
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

/// A completed diagnosis whose overall outcome is not `ok`.
///
/// It travels as the command's error only so the CLI entry point can turn it
/// into the outcome's exit status after the report is printed. It is not a
/// failure of the doctor: the entry point must not add an `Error:` report or a
/// dogfood fix prompt for it ([`workflow_doctor_exit_status`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkflowDoctorUnhealthy {
    pub outcome: WorkflowDoctorOutcome,
}

impl std::fmt::Display for WorkflowDoctorUnhealthy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "workflow doctor outcome={} (exit {})",
            self.outcome.as_str(),
            self.outcome.exit_code()
        )
    }
}

impl std::error::Error for WorkflowDoctorUnhealthy {}

/// The exit status a [`run`] error carries when it is a completed, non-`ok`
/// diagnosis; `None` for a real failure (which exits `1`).
pub fn workflow_doctor_exit_status(err: &anyhow::Error) -> Option<u8> {
    err.downcast_ref::<WorkflowDoctorUnhealthy>()
        .map(|unhealthy| unhealthy.outcome.exit_code())
}

/// Print the report, then report its outcome as the result: `Ok(())` only for
/// an overall `ok` outcome, otherwise a [`WorkflowDoctorUnhealthy`] error whose
/// exit status is [`WorkflowDoctorOutcome::exit_code`].
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
    outcome_result(report.outcome)
}

/// `Ok(())` for an `ok` outcome, otherwise a [`WorkflowDoctorUnhealthy`] error
/// carrying it. Shared by `doctor` and `autofix` so both commands exit with the
/// same status for the same outcome.
pub(crate) fn outcome_result(outcome: WorkflowDoctorOutcome) -> Result<()> {
    match outcome {
        WorkflowDoctorOutcome::Ok => Ok(()),
        outcome => Err(WorkflowDoctorUnhealthy { outcome }.into()),
    }
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
    let document = read_document_facts(file, &mut warnings);
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
            document,
        },
        warnings,
    ))
}

fn read_document_facts(file: &Path, warnings: &mut Vec<String>) -> DocumentDoctorFacts {
    match std::fs::read_to_string(file) {
        Ok(content) => document_doctor_facts_from_content(&content),
        Err(err) => {
            warnings.push(format!("document queue inspection unavailable: {err}"));
            DocumentDoctorFacts::default()
        }
    }
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
        lazily_current_diverges,
    }
}

fn read_json(path: &Path) -> Result<Value> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    Ok(serde_json::from_str(&content)?)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use agent_doc_workflow::invariants::WorkflowInvariantId;

    /// Live effects for a healthy, freshly initialized project: session-check is
    /// ok, the controller answers with no actor for the document, and the editor
    /// projection is converged.
    pub(crate) struct FreshProjectEffects;

    /// [`FreshProjectEffects`] whose live editor authority is ahead of disk,
    /// which `editor_convergence` reports `recoverable`.
    pub(crate) struct DivergedEditorEffects;

    impl WorkflowDoctorEffects for DivergedEditorEffects {
        fn inspect_session_check(&mut self, file: &Path) -> Result<Option<LiveSessionCheckFacts>> {
            FreshProjectEffects.inspect_session_check(file)
        }

        fn inspect_actor(&mut self, project_root: &Path, file: &Path) -> Result<ActorDoctorFacts> {
            FreshProjectEffects.inspect_actor(project_root, file)
        }

        fn lazily_current_diverges(
            &mut self,
            _file: &Path,
            _disk_content: &str,
            _project_root: &Path,
        ) -> Result<Option<bool>> {
            Ok(Some(true))
        }
    }

    impl WorkflowDoctorEffects for FreshProjectEffects {
        fn inspect_session_check(&mut self, _file: &Path) -> Result<Option<LiveSessionCheckFacts>> {
            Ok(Some(LiveSessionCheckFacts {
                ok: Some(true),
                status: Some("ok".to_string()),
                message: Some("no open cycle".to_string()),
                warnings: Vec::new(),
            }))
        }

        fn inspect_actor(
            &mut self,
            _project_root: &Path,
            _file: &Path,
        ) -> Result<ActorDoctorFacts> {
            Ok(ActorDoctorFacts {
                inspection_available: true,
                record_present: false,
                controller_fresh: Some(true),
                ..ActorDoctorFacts::default()
            })
        }

        fn lazily_current_diverges(
            &mut self,
            _file: &Path,
            _disk_content: &str,
            _project_root: &Path,
        ) -> Result<Option<bool>> {
            Ok(Some(false))
        }
    }

    pub(crate) fn fresh_project(document: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let status = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(root)
            .status()
            .unwrap();
        assert!(status.success());
        let file = root.join("plan.md");
        std::fs::write(&file, document).unwrap();
        (dir, file)
    }

    #[test]
    fn diagnose_fresh_document_without_queue_is_healthy() {
        // Onboarding regression: `agent-doc init` + a brand-new session document
        // with no `agent:queue` used to report `outcome=blocked (queue_continuation)`.
        let (_dir, file) = fresh_project(
            "---\nagent_doc_session: fresh\nagent: claude\n---\n\n# Session: Untitled Session\n\n## User\n\n",
        );

        let report = diagnose(
            &file,
            &WorkflowDoctorOptions::default(),
            &mut FreshProjectEffects,
        )
        .unwrap();

        assert_eq!(report.facts.document.queue_component_count, Some(0));
        assert_eq!(
            report.facts.git.snapshot_status.as_deref(),
            Some("NoSnapshot")
        );
        assert!(!report.facts.cycle_state.present);
        for result in &report.results {
            assert_eq!(result.outcome, WorkflowDoctorOutcome::Ok, "{result:?}");
        }
        assert_eq!(report.outcome, WorkflowDoctorOutcome::Ok, "{report:?}");
    }

    #[test]
    fn diagnose_document_with_live_queue_and_no_preflight_stays_blocked() {
        let (_dir, file) = fresh_project(
            "---\nagent_doc_session: fresh\nagent: claude\n---\n\n# Session\n\n<!-- agent:queue -->\n- ship the fix\n<!-- /agent:queue -->\n",
        );

        let report = diagnose(
            &file,
            &WorkflowDoctorOptions::default(),
            &mut FreshProjectEffects,
        )
        .unwrap();

        assert_eq!(report.facts.document.queue_component_count, Some(1));
        let queue = report
            .results
            .iter()
            .find(|result| result.id == WorkflowInvariantId::QueueContinuation)
            .unwrap();
        assert_eq!(queue.outcome, WorkflowDoctorOutcome::Blocked, "{queue:?}");
        assert_eq!(report.outcome, WorkflowDoctorOutcome::Blocked);
    }

    pub(crate) const FRESH_DOCUMENT: &str = "---\nagent_doc_session: fresh\nagent: claude\n---\n\n# Session: Untitled Session\n\n## User\n\n";
    pub(crate) const QUEUED_DOCUMENT: &str = "---\nagent_doc_session: fresh\nagent: claude\n---\n\n# Session\n\n<!-- agent:queue -->\n- ship the fix\n<!-- /agent:queue -->\n";

    /// `run` resolves to the exit status the CLI returns.
    fn run_exit_status(file: &Path, effects: &mut impl WorkflowDoctorEffects) -> u8 {
        let options = WorkflowDoctorOptions {
            json: true,
            ..WorkflowDoctorOptions::default()
        };
        match run(file, options, effects) {
            Ok(()) => 0,
            Err(err) => workflow_doctor_exit_status(&err)
                .unwrap_or_else(|| panic!("a completed diagnosis is not a failure: {err:#}")),
        }
    }

    #[test]
    fn run_exits_zero_for_a_healthy_document() {
        let (_dir, file) = fresh_project(FRESH_DOCUMENT);
        assert_eq!(run_exit_status(&file, &mut FreshProjectEffects), 0);
    }

    #[test]
    fn run_exits_recoverable_status_when_a_safe_repair_exists() {
        let (_dir, file) = fresh_project(FRESH_DOCUMENT);
        let report = diagnose(
            &file,
            &WorkflowDoctorOptions::default(),
            &mut DivergedEditorEffects,
        )
        .unwrap();
        assert_eq!(
            report.outcome,
            WorkflowDoctorOutcome::Recoverable,
            "{report:?}"
        );
        assert_eq!(
            run_exit_status(&file, &mut DivergedEditorEffects),
            agent_doc_workflow::doctor::WORKFLOW_DOCTOR_EXIT_RECOVERABLE
        );
    }

    #[test]
    fn run_exits_operator_status_when_a_person_must_act() {
        let (dir, file) = fresh_project(FRESH_DOCUMENT);
        let logs = dir.path().join(".agent-doc/logs");
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::write(logs.join("ops.log"), "File Cache Conflict\n").unwrap();
        let report = diagnose(
            &file,
            &WorkflowDoctorOptions::default(),
            &mut FreshProjectEffects,
        )
        .unwrap();
        assert_eq!(
            report.outcome,
            WorkflowDoctorOutcome::Operator,
            "{report:?}"
        );
        assert_eq!(
            run_exit_status(&file, &mut FreshProjectEffects),
            agent_doc_workflow::doctor::WORKFLOW_DOCTOR_EXIT_OPERATOR
        );
    }

    #[test]
    fn run_exits_blocked_status_when_evidence_is_missing() {
        let (_dir, file) = fresh_project(QUEUED_DOCUMENT);
        assert_eq!(
            run_exit_status(&file, &mut FreshProjectEffects),
            agent_doc_workflow::doctor::WORKFLOW_DOCTOR_EXIT_BLOCKED
        );
    }

    #[test]
    fn a_doctor_that_failed_to_run_carries_no_outcome_status() {
        let (dir, _file) = fresh_project(FRESH_DOCUMENT);
        let err = run(
            &dir.path().join("missing.md"),
            WorkflowDoctorOptions::default(),
            &mut FreshProjectEffects,
        )
        .unwrap_err();
        assert_eq!(workflow_doctor_exit_status(&err), None, "{err:#}");
    }
}
