//! Workflow invariant doctor for agent-doc session documents.
//!
//! The doctor is a diagnostic surface. It gathers durable facts from the
//! current document, optional preflight/session-check JSON captures, cycle state,
//! ops logs, controller inspection, git state, and editor sidecars, then
//! evaluates the workflow invariant catalog into typed outcomes.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

use crate::flow::workflow_invariants::{
    RemediationAction, WorkflowInvariant, WorkflowInvariantCatalog, WorkflowInvariantId,
    workflow_invariant_catalog,
};

pub const WORKFLOW_DOCTOR_SCHEMA_VERSION: u8 = 1;
const DEFAULT_OPS_LIMIT: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowDoctorOutcome {
    Ok,
    Recoverable,
    Operator,
    Blocked,
}

impl WorkflowDoctorOutcome {
    fn rank(self) -> u8 {
        match self {
            Self::Ok => 0,
            Self::Recoverable => 1,
            Self::Operator => 2,
            Self::Blocked => 3,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Recoverable => "recoverable",
            Self::Operator => "operator",
            Self::Blocked => "blocked",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDoctorReport {
    pub schema_version: u8,
    pub file: String,
    pub outcome: WorkflowDoctorOutcome,
    pub catalog_contract_version: String,
    pub facts: WorkflowDoctorFacts,
    pub results: Vec<WorkflowInvariantResult>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowDoctorFacts {
    pub project_root: Option<String>,
    pub document_hash: Option<String>,
    pub preflight: PreflightDoctorFacts,
    pub session_check: SessionCheckDoctorFacts,
    pub cycle_state: CycleStateDoctorFacts,
    pub ops_log: OpsLogDoctorFacts,
    pub actor: ActorDoctorFacts,
    pub git: GitDoctorFacts,
    pub editor: EditorDoctorFacts,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PreflightDoctorFacts {
    pub json_provided: bool,
    pub queue_active: Option<bool>,
    pub queue_continuation_required: Option<bool>,
    pub queue_drainable_head_count: Option<usize>,
    pub queue_prompts: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionCheckDoctorFacts {
    pub json_provided: bool,
    pub ok: Option<bool>,
    pub status: Option<String>,
    pub message: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CycleStateDoctorFacts {
    pub present: bool,
    pub cycle_id: Option<String>,
    pub phase: Option<String>,
    pub last_event: Option<String>,
    pub open: Option<bool>,
    pub capture_id: Option<String>,
    pub response_sha256: Option<String>,
    pub queue_task_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpsLogDoctorFacts {
    pub present: bool,
    pub scanned_lines: usize,
    pub markers: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActorDoctorFacts {
    pub inspection_available: bool,
    pub state: Option<String>,
    pub generation: Option<u64>,
    pub pane: Option<String>,
    pub supervisor_pid: Option<u32>,
    pub controller_fresh: Option<bool>,
    pub supervisor_fresh: Option<bool>,
    pub guidance: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GitDoctorFacts {
    pub snapshot_status: Option<String>,
    pub tracked_modified_paths: Vec<String>,
    pub parent_gitlink_stale: bool,
    pub parent_gitlink_path: Option<String>,
    pub parent_gitlink_parent_head: Option<String>,
    pub parent_gitlink_submodule_head: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EditorDoctorFacts {
    pub patches_dir_present: bool,
    pub ack_content_dir_present: bool,
    pub live_buffer_dir_present: bool,
    pub live_buffer_diverges: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowInvariantResult {
    pub id: WorkflowInvariantId,
    pub title: String,
    pub outcome: WorkflowDoctorOutcome,
    pub evidence: Vec<String>,
    pub missing_fact_sources: Vec<String>,
    pub disproof_markers: Vec<String>,
    pub repair_commands: Vec<String>,
    pub operator_actions: Vec<String>,
}

impl WorkflowInvariantResult {
    fn new(invariant: &WorkflowInvariant) -> Self {
        Self {
            id: invariant.id,
            title: invariant.title.clone(),
            outcome: WorkflowDoctorOutcome::Ok,
            evidence: Vec::new(),
            missing_fact_sources: Vec::new(),
            disproof_markers: Vec::new(),
            repair_commands: Vec::new(),
            operator_actions: Vec::new(),
        }
    }

    fn blocked_missing(mut self, source: impl Into<String>, command: impl Into<String>) -> Self {
        self.outcome = WorkflowDoctorOutcome::Blocked;
        self.missing_fact_sources.push(source.into());
        self.repair_commands.push(command.into());
        self
    }

    fn recoverable(mut self, evidence: impl Into<String>, commands: Vec<String>) -> Self {
        self.outcome = WorkflowDoctorOutcome::Recoverable;
        self.evidence.push(evidence.into());
        self.repair_commands.extend(commands);
        self
    }

    fn operator(mut self, evidence: impl Into<String>, actions: Vec<String>) -> Self {
        self.outcome = WorkflowDoctorOutcome::Operator;
        self.evidence.push(evidence.into());
        self.operator_actions.extend(actions);
        self
    }

    fn ok(mut self, evidence: impl Into<String>) -> Self {
        self.outcome = WorkflowDoctorOutcome::Ok;
        self.evidence.push(evidence.into());
        self
    }
}

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

pub fn evaluate_catalog(
    file: &Path,
    catalog: WorkflowInvariantCatalog,
    facts: WorkflowDoctorFacts,
    warnings: Vec<String>,
) -> WorkflowDoctorReport {
    let mut results = Vec::new();
    for invariant in &catalog.invariants {
        let result = match invariant.id {
            WorkflowInvariantId::QueueContinuation => {
                evaluate_queue_continuation(invariant, &facts, file)
            }
            WorkflowInvariantId::StaleSupervisor => evaluate_stale_supervisor(invariant, &facts),
            WorkflowInvariantId::CloseoutCommit => {
                evaluate_closeout_commit(invariant, &facts, file)
            }
            WorkflowInvariantId::EditorConvergence => {
                evaluate_editor_convergence(invariant, &facts)
            }
            WorkflowInvariantId::GenerationRedirect => {
                evaluate_generation_redirect(invariant, &facts)
            }
            WorkflowInvariantId::ParentGitlink => evaluate_parent_gitlink(invariant, &facts),
        };
        results.push(result);
    }
    let outcome = results
        .iter()
        .map(|result| result.outcome)
        .max_by_key(|outcome| outcome.rank())
        .unwrap_or(WorkflowDoctorOutcome::Ok);
    WorkflowDoctorReport {
        schema_version: WORKFLOW_DOCTOR_SCHEMA_VERSION,
        file: file.display().to_string(),
        outcome,
        catalog_contract_version: catalog.contract_version,
        facts,
        results,
        warnings,
    }
}

fn gather_facts(
    file: &Path,
    options: &WorkflowDoctorOptions,
) -> Result<(WorkflowDoctorFacts, Vec<String>)> {
    let mut warnings = Vec::new();
    let canonical = file
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", file.display()))?;
    let project_root = crate::fs_util::find_project_root(&canonical);
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
    let mut facts = PreflightDoctorFacts {
        json_provided: true,
        queue_active: lookup_bool(&value, "queue_active"),
        queue_continuation_required: lookup_bool(&value, "queue_continuation_required"),
        queue_drainable_head_count: lookup_usize(&value, "queue_drainable_head_count"),
        queue_prompts: lookup_string_array(&value, "queue_prompts"),
        warnings: Vec::new(),
    };
    facts.warnings = lookup_string_array(&value, "warnings");
    Ok(facts)
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
    Ok(SessionCheckDoctorFacts {
        json_provided: true,
        ok: lookup_bool(&value, "ok"),
        status: lookup_string(&value, "status"),
        message: lookup_string(&value, "message")
            .or_else(|| lookup_string(&value, "reason"))
            .or_else(|| lookup_string(&value, "detail")),
        warnings: lookup_string_array(&value, "warnings"),
    })
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
                queue_task_id: state.queue_task_id,
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
    let content = match crate::fs_util::read_optional_text(&path) {
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

fn evaluate_queue_continuation(
    invariant: &WorkflowInvariant,
    facts: &WorkflowDoctorFacts,
    file: &Path,
) -> WorkflowInvariantResult {
    let result = WorkflowInvariantResult::new(invariant);
    if !facts.preflight.json_provided {
        return result.blocked_missing(
            "preflight_json.queue_continuation_required",
            format!(
                "agent-doc preflight {} --probe > /tmp/agent-doc-preflight.json",
                file.display()
            ),
        );
    }
    match (
        facts.preflight.queue_active,
        facts.preflight.queue_continuation_required,
        facts.preflight.queue_drainable_head_count,
    ) {
        (Some(true), Some(false), Some(count)) if count > 0 => result.recoverable(
            "preflight reports a drainable queue but queue_continuation_required=false",
            vec!["agent-doc drain-claim <FILE> && agent-doc <FILE>".to_string()],
        ),
        (Some(true), Some(false), Some(0)) => result.operator(
            "active queue has no drainable head; remaining heads are operator-only or inert",
            operator_steps(invariant),
        ),
        (Some(true), Some(true), Some(count)) if count > 0 => result.ok(format!(
            "queue continuation is required and {count} drainable head(s) are visible"
        )),
        (Some(false), _, _) => result.ok("queue is inactive"),
        _ => result.blocked_missing(
            "preflight_json.queue_active/queue_continuation_required/queue_drainable_head_count",
            format!(
                "agent-doc preflight {} --probe > /tmp/agent-doc-preflight.json",
                file.display()
            ),
        ),
    }
}

fn evaluate_stale_supervisor(
    invariant: &WorkflowInvariant,
    facts: &WorkflowDoctorFacts,
) -> WorkflowInvariantResult {
    let mut result = WorkflowInvariantResult::new(invariant);
    if has_marker(&facts.ops_log, "stale_queue_pause") {
        result
            .disproof_markers
            .push("ops.log contains legacy stale queue pause / stale host supervisor".to_string());
        return result.recoverable(
            "stale supervisor queue pause surfaced in ops log",
            safe_commands(invariant),
        );
    }
    if facts.actor.controller_fresh == Some(false) || facts.actor.supervisor_fresh == Some(false) {
        return result.recoverable(
            "controller or route-owned supervisor binary freshness is stale",
            safe_commands(invariant),
        );
    }
    if facts.actor.controller_fresh == Some(true) && facts.actor.supervisor_fresh.unwrap_or(true) {
        return result.ok("controller/supervisor freshness matches installed binary");
    }
    if !facts.actor.inspection_available {
        return result.blocked_missing(
            "actor_state.freshness",
            "agent-doc admin inspect <FILE> --json",
        );
    }
    result.ok("no stale supervisor markers found")
}

fn evaluate_closeout_commit(
    invariant: &WorkflowInvariant,
    facts: &WorkflowDoctorFacts,
    file: &Path,
) -> WorkflowInvariantResult {
    let result = WorkflowInvariantResult::new(invariant);
    if facts.session_check.ok == Some(false) {
        return result.recoverable(
            facts
                .session_check
                .message
                .clone()
                .unwrap_or_else(|| "session-check reports interrupted closeout".to_string()),
            closeout_repair_commands(file, facts),
        );
    }
    if facts.cycle_state.open == Some(true) {
        return result.recoverable(
            format!(
                "cycle_state phase={} is still open",
                facts.cycle_state.phase.as_deref().unwrap_or("unknown")
            ),
            closeout_repair_commands(file, facts),
        );
    }
    if facts.git.snapshot_status.as_deref().is_some_and(|status| {
        status.contains("SnapshotDiffersFromHead") || status == "NoSnapshot" || status == "NoHead"
    }) {
        return result.recoverable(
            format!(
                "snapshot git status is {}",
                facts.git.snapshot_status.as_deref().unwrap_or("unknown")
            ),
            vec![format!("agent-doc session-check {}", file.display())],
        );
    }
    if facts.session_check.ok == Some(true) {
        return result.ok("session-check reports ok and no open cycle is present");
    }
    result.blocked_missing(
        "session_check.ok",
        format!("agent-doc session-check {}", file.display()),
    )
}

fn evaluate_editor_convergence(
    invariant: &WorkflowInvariant,
    facts: &WorkflowDoctorFacts,
) -> WorkflowInvariantResult {
    let mut result = WorkflowInvariantResult::new(invariant);
    if facts.editor.live_buffer_diverges == Some(true) {
        result
            .disproof_markers
            .push("live editor buffer diverges from disk".to_string());
        return result.operator(
            "editor proof sidecar reports a live unsaved buffer ahead of disk",
            vec!["save the live editor buffer or let the plugin handle save_document IPC, then rerun agent-doc doctor <FILE>".to_string()],
        );
    }
    if has_marker(&facts.ops_log, "editor_convergence_failure") {
        result
            .disproof_markers
            .push("ops.log contains IPC/file-cache/editor convergence failure marker".to_string());
        return result.operator(
            "editor convergence failure marker found in ops log",
            operator_steps(invariant),
        );
    }
    if facts.editor.patches_dir_present || facts.editor.ack_content_dir_present {
        return result
            .ok("editor IPC sidecar directories are present and no failure markers were found");
    }
    result.blocked_missing(
        "editor_proof_sidecar",
        "open the managed document in an editor with the agent-doc plugin or rerun with --force-disk only when no live editor owns the file",
    )
}

fn evaluate_generation_redirect(
    invariant: &WorkflowInvariant,
    facts: &WorkflowDoctorFacts,
) -> WorkflowInvariantResult {
    let mut result = WorkflowInvariantResult::new(invariant);
    if has_marker(&facts.ops_log, "stale_generation_block") {
        result
            .disproof_markers
            .push("ops.log contains stale generation dispatch block".to_string());
        return result.recoverable(
            "stale generation dispatch can retry on the current generation",
            vec!["agent-doc admin inspect <FILE> --json && agent-doc route <FILE>".to_string()],
        );
    }
    if has_marker(&facts.ops_log, "retry_on_current_generation") {
        return result.ok("ops.log records retry_on_current_generation proof");
    }
    if facts.actor.generation.is_some() {
        return result.ok("current actor generation is inspectable");
    }
    result.blocked_missing(
        "actor_state.generation",
        "agent-doc admin inspect <FILE> --json",
    )
}

fn evaluate_parent_gitlink(
    invariant: &WorkflowInvariant,
    facts: &WorkflowDoctorFacts,
) -> WorkflowInvariantResult {
    let result = WorkflowInvariantResult::new(invariant);
    if facts.git.parent_gitlink_stale {
        let path = facts
            .git
            .parent_gitlink_path
            .as_deref()
            .unwrap_or("src/agent-doc");
        return result.recoverable(
            format!("parent gitlink for {path} differs from submodule HEAD"),
            vec![format!(
                "git add {path} && git commit -m 'agent-doc: update workflow doctor'"
            )],
        );
    }
    result.ok("parent gitlink matches submodule HEAD or document is not inside a submodule")
}

fn closeout_repair_commands(file: &Path, facts: &WorkflowDoctorFacts) -> Vec<String> {
    let phase = facts.cycle_state.phase.as_deref().unwrap_or_default();
    if phase == "ResponseCaptured" || facts.cycle_state.response_sha256.is_some() {
        vec![format!("agent-doc write --commit {}", file.display())]
    } else {
        vec![
            format!("agent-doc preflight {} --probe", file.display()),
            format!("agent-doc session-check {}", file.display()),
        ]
    }
}

fn safe_commands(invariant: &WorkflowInvariant) -> Vec<String> {
    invariant
        .safe_remediation
        .iter()
        .filter_map(|step| step.command.clone())
        .collect()
}

fn operator_steps(invariant: &WorkflowInvariant) -> Vec<String> {
    invariant
        .operator_gated_remediation
        .iter()
        .map(|step| remediation_label(step.action, step.command.as_deref()))
        .collect()
}

fn remediation_label(action: RemediationAction, command: Option<&str>) -> String {
    match command {
        Some(command) => command.to_string(),
        None => format!("{action:?}"),
    }
}

fn read_json(path: &Path) -> Result<Value> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    Ok(serde_json::from_str(&content)?)
}

fn lookup_value<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value
        .get(key)
        .or_else(|| value.get("report").and_then(|report| report.get(key)))
        .or_else(|| {
            value
                .get("session_check")
                .and_then(|report| report.get(key))
        })
}

fn lookup_bool(value: &Value, key: &str) -> Option<bool> {
    lookup_value(value, key).and_then(Value::as_bool)
}

fn lookup_usize(value: &Value, key: &str) -> Option<usize> {
    lookup_value(value, key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
}

fn lookup_string(value: &Value, key: &str) -> Option<String> {
    lookup_value(value, key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn lookup_string_array(value: &Value, key: &str) -> Vec<String> {
    lookup_value(value, key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn classify_ops_marker(line: &str) -> Option<&'static str> {
    if line.contains("failed_stage=queue_paused")
        && (line.contains("reason=#qchurn")
            || line.contains("stale host supervisor")
            || line.contains("supervisor_binary_stale"))
    {
        return Some("stale_queue_pause");
    }
    if line.contains("supervisor_binary_stale") || line.contains("stale host supervisor") {
        return Some("stale_supervisor");
    }
    if line.contains("retry_on_current_generation") || line.contains("supervisor_restart_redirect")
    {
        return Some("retry_on_current_generation");
    }
    if line.contains("stale_generation") || line.contains("stale generation") {
        return Some("stale_generation_block");
    }
    if line.contains("File Cache Conflict")
        || line.contains("ipc_proof_insufficient")
        || line.contains("editor_convergence_ack_mismatch")
        || line.contains("editor_convergence_no_ack")
        || line.contains("live_prompt_drift_after_preflight")
    {
        return Some("editor_convergence_failure");
    }
    None
}

fn has_marker(ops: &OpsLogDoctorFacts, marker: &str) -> bool {
    ops.markers.iter().any(|item| item == marker)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cycle_state::CyclePhase;

    fn invariant_result(
        report: &WorkflowDoctorReport,
        id: WorkflowInvariantId,
    ) -> &WorkflowInvariantResult {
        report
            .results
            .iter()
            .find(|result| result.id == id)
            .expect("invariant result")
    }

    #[test]
    fn doctor_marks_closeout_recoverable_for_captured_response() {
        let mut facts = WorkflowDoctorFacts::default();
        facts.session_check.ok = Some(false);
        facts.session_check.status = Some("interrupted".to_string());
        facts.cycle_state.present = true;
        facts.cycle_state.phase = Some(format!("{:?}", CyclePhase::ResponseCaptured));
        facts.cycle_state.open = Some(true);
        facts.cycle_state.response_sha256 = Some("abc".to_string());

        let report = evaluate_catalog(
            Path::new("tasks/example.md"),
            workflow_invariant_catalog(),
            facts,
            Vec::new(),
        );

        let closeout = invariant_result(&report, WorkflowInvariantId::CloseoutCommit);
        assert_eq!(closeout.outcome, WorkflowDoctorOutcome::Recoverable);
        assert!(
            closeout
                .repair_commands
                .iter()
                .any(|command| command == "agent-doc write --commit tasks/example.md"),
            "{closeout:?}"
        );
    }

    #[test]
    fn doctor_marks_parent_gitlink_recoverable_when_stale() {
        let mut facts = WorkflowDoctorFacts::default();
        facts.session_check.ok = Some(true);
        facts.git.parent_gitlink_stale = true;
        facts.git.parent_gitlink_path = Some("src/agent-doc".to_string());

        let report = evaluate_catalog(
            Path::new("tasks/example.md"),
            workflow_invariant_catalog(),
            facts,
            Vec::new(),
        );

        let parent = invariant_result(&report, WorkflowInvariantId::ParentGitlink);
        assert_eq!(parent.outcome, WorkflowDoctorOutcome::Recoverable);
        assert_eq!(
            parent.repair_commands,
            vec!["git add src/agent-doc && git commit -m 'agent-doc: update workflow doctor'"]
        );
    }

    #[test]
    fn doctor_blocks_queue_continuation_without_preflight_json() {
        let facts = WorkflowDoctorFacts::default();
        let report = evaluate_catalog(
            Path::new("tasks/example.md"),
            workflow_invariant_catalog(),
            facts,
            Vec::new(),
        );

        let queue = invariant_result(&report, WorkflowInvariantId::QueueContinuation);
        assert_eq!(queue.outcome, WorkflowDoctorOutcome::Blocked);
        assert!(
            queue
                .missing_fact_sources
                .contains(&"preflight_json.queue_continuation_required".to_string())
        );
    }
}
