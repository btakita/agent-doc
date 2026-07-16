//! Pure workflow doctor policy.
//!
//! Orchestration gathers logs and other evidence. This module owns the durable
//! doctor schema and classification rules that turn raw workflow evidence into
//! stable invariant outcomes.

use serde::{Deserialize, Serialize};
use std::{fmt::Write as _, path::Path};

use crate::invariants::{
    RemediationAction, WorkflowInvariant, WorkflowInvariantCatalog, WorkflowInvariantId,
};

pub const WORKFLOW_DOCTOR_SCHEMA_VERSION: u8 = 2;

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
    pub baseline_file: Option<String>,
    pub prompt_targets: Vec<String>,
    pub queue_task_id: Option<String>,
    pub turn_id: Option<String>,
    pub had_pending_mutations: bool,
    pub pending_done_ids: Vec<String>,
    pub pending_gated_ids: Vec<String>,
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
    pub legacy_ack_content_dir_present: bool,
    pub legacy_live_buffer_dir_present: bool,
    pub lazily_current_diverges: Option<bool>,
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

pub fn format_text_report(report: &WorkflowDoctorReport) -> String {
    let mut output = String::new();
    writeln!(
        &mut output,
        "workflow doctor: {} outcome={}",
        report.file,
        report.outcome.as_str()
    )
    .expect("write workflow doctor report header");
    for warning in &report.warnings {
        writeln!(&mut output, "warning: {warning}").expect("write workflow doctor warning");
    }
    for result in &report.results {
        writeln!(
            &mut output,
            "- {} outcome={} title={}",
            result.id,
            result.outcome.as_str(),
            result.title
        )
        .expect("write workflow doctor result");
        for evidence in &result.evidence {
            writeln!(&mut output, "  evidence: {evidence}")
                .expect("write workflow doctor evidence");
        }
        for missing in &result.missing_fact_sources {
            writeln!(&mut output, "  missing: {missing}")
                .expect("write workflow doctor missing fact");
        }
        for marker in &result.disproof_markers {
            writeln!(&mut output, "  disproof: {marker}")
                .expect("write workflow doctor disproof marker");
        }
        for command in &result.repair_commands {
            writeln!(&mut output, "  repair: {command}")
                .expect("write workflow doctor repair command");
        }
        for action in &result.operator_actions {
            writeln!(&mut output, "  operator: {action}")
                .expect("write workflow doctor operator action");
        }
    }
    output
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
        (_, Some(false), Some(count)) if count > 0 => result.recoverable(
            "preflight reports a drainable queue but queue_continuation_required=false",
            vec!["agent-doc drain-claim <FILE> && agent-doc <FILE>".to_string()],
        ),
        (Some(true), Some(false), Some(0)) => result.operator(
            "active queue has no drainable head; remaining heads are operator-only or inert",
            operator_steps(invariant),
        ),
        (_, Some(false), Some(0)) => {
            result.ok("queue continuation is not required and no drainable heads are visible")
        }
        (_, Some(true), Some(count)) if count > 0 => result.ok(format!(
            "queue continuation is required and {count} drainable head(s) are visible"
        )),
        (Some(false), _, _) => result.ok("queue is inactive"),
        _ => result.blocked_missing(
            "preflight_json.queue_continuation_required/queue_drainable_head_count",
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
    if facts.editor.lazily_current_diverges == Some(true) {
        result
            .disproof_markers
            .push("live editor buffer diverges from disk".to_string());
        return result.operator(
            "the Lazily/CPC current document is ahead of disk",
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
    if facts.editor.legacy_ack_content_dir_present {
        result
            .disproof_markers
            .push("legacy ack-content directory is ignored as editor proof".to_string());
    }
    if facts.editor.legacy_live_buffer_dir_present {
        result
            .disproof_markers
            .push("legacy live-buffer directory is ignored as editor proof".to_string());
    }
    if facts.editor.lazily_current_diverges == Some(false) {
        return result
            .ok("Lazily/CPC current is converged with disk and no failure markers were found");
    }
    result.blocked_missing(
        "editor_lazily_projection",
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

fn has_marker(ops: &OpsLogDoctorFacts, marker: &str) -> bool {
    ops.markers.iter().any(|item| item == marker)
}

pub fn ops_log_facts_from_content(content: &str, limit: usize) -> OpsLogDoctorFacts {
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

pub fn classify_ops_marker(line: &str) -> Option<&'static str> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::invariants::{WorkflowInvariantId, workflow_invariant_catalog};
    use std::path::Path;

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
        facts.cycle_state.phase = Some("ResponseCaptured".to_string());
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

    #[test]
    fn doctor_accepts_inactive_queue_without_legacy_queue_active_fact() {
        let mut facts = WorkflowDoctorFacts::default();
        facts.preflight.json_provided = true;
        facts.preflight.queue_continuation_required = Some(false);
        facts.preflight.queue_drainable_head_count = Some(0);

        let report = evaluate_catalog(
            Path::new("tasks/example.md"),
            workflow_invariant_catalog(),
            facts,
            Vec::new(),
        );

        let queue = invariant_result(&report, WorkflowInvariantId::QueueContinuation);
        assert_eq!(queue.outcome, WorkflowDoctorOutcome::Ok);
        assert!(queue.evidence.contains(
            &"queue continuation is not required and no drainable heads are visible".to_string()
        ));
    }

    #[test]
    fn doctor_marks_stale_supervisor_recoverable_from_actor_freshness() {
        let mut facts = WorkflowDoctorFacts::default();
        facts.actor.inspection_available = true;
        facts.actor.controller_fresh = Some(false);

        let report = evaluate_catalog(
            Path::new("tasks/example.md"),
            workflow_invariant_catalog(),
            facts,
            Vec::new(),
        );

        let stale = invariant_result(&report, WorkflowInvariantId::StaleSupervisor);
        assert_eq!(stale.outcome, WorkflowDoctorOutcome::Recoverable);
        assert!(
            stale
                .evidence
                .iter()
                .any(|item| item.contains("binary freshness is stale")),
            "{stale:?}"
        );
    }

    #[test]
    fn doctor_marks_editor_convergence_operator_for_lazily_drift() {
        let mut facts = WorkflowDoctorFacts::default();
        facts.editor.lazily_current_diverges = Some(true);

        let report = evaluate_catalog(
            Path::new("tasks/example.md"),
            workflow_invariant_catalog(),
            facts,
            Vec::new(),
        );

        let editor = invariant_result(&report, WorkflowInvariantId::EditorConvergence);
        assert_eq!(editor.outcome, WorkflowDoctorOutcome::Operator);
        assert!(
            editor
                .disproof_markers
                .contains(&"live editor buffer diverges from disk".to_string())
        );
    }

    #[test]
    fn doctor_marks_generation_redirect_recoverable_for_stale_generation_marker() {
        let mut facts = WorkflowDoctorFacts::default();
        facts.ops_log.markers = vec!["stale_generation_block".to_string()];

        let report = evaluate_catalog(
            Path::new("tasks/example.md"),
            workflow_invariant_catalog(),
            facts,
            Vec::new(),
        );

        let generation = invariant_result(&report, WorkflowInvariantId::GenerationRedirect);
        assert_eq!(generation.outcome, WorkflowDoctorOutcome::Recoverable);
        assert!(generation.repair_commands.contains(
            &"agent-doc admin inspect <FILE> --json && agent-doc route <FILE>".to_string()
        ));
    }

    #[test]
    fn ops_marker_classifies_queue_pause_causes() {
        assert_eq!(
            classify_ops_marker("failed_stage=queue_paused reason=#qchurn"),
            Some("stale_queue_pause")
        );
        assert_eq!(
            classify_ops_marker("failed_stage=queue_paused stale host supervisor"),
            Some("stale_queue_pause")
        );
        assert_eq!(
            classify_ops_marker("failed_stage=queue_paused supervisor_binary_stale"),
            Some("stale_queue_pause")
        );
    }

    #[test]
    fn ops_marker_classifies_stale_supervisor_without_queue_pause() {
        assert_eq!(
            classify_ops_marker("supervisor_binary_stale path=/tmp/agent-doc"),
            Some("stale_supervisor")
        );
        assert_eq!(
            classify_ops_marker("stale host supervisor generation=7"),
            Some("stale_supervisor")
        );
    }

    #[test]
    fn ops_marker_classifies_generation_redirect_and_blocks() {
        assert_eq!(
            classify_ops_marker("retry_on_current_generation supervisor restarted"),
            Some("retry_on_current_generation")
        );
        assert_eq!(
            classify_ops_marker("supervisor_restart_redirect generation=9"),
            Some("retry_on_current_generation")
        );
        assert_eq!(
            classify_ops_marker("blocked stale_generation current=4"),
            Some("stale_generation_block")
        );
        assert_eq!(
            classify_ops_marker("blocked stale generation current=4"),
            Some("stale_generation_block")
        );
    }

    #[test]
    fn ops_marker_classifies_editor_convergence_failures() {
        for line in [
            "File Cache Conflict",
            "ipc_proof_insufficient",
            "editor_convergence_ack_mismatch",
            "editor_convergence_no_ack",
            "live_prompt_drift_after_preflight",
        ] {
            assert_eq!(
                classify_ops_marker(line),
                Some("editor_convergence_failure"),
                "{line}"
            );
        }
    }

    #[test]
    fn ops_marker_ignores_unrelated_lines() {
        assert_eq!(classify_ops_marker("plain ops log line"), None);
    }

    #[test]
    fn ops_log_facts_scan_recent_lines_and_dedup_markers() {
        let facts = ops_log_facts_from_content(
            "blocked stale_generation current=4\nplain\nFile Cache Conflict\nFile Cache Conflict\n",
            3,
        );

        assert!(facts.present);
        assert_eq!(facts.scanned_lines, 3);
        assert_eq!(facts.markers, vec!["editor_convergence_failure"]);
    }

    #[test]
    fn text_report_formatter_includes_all_result_sections() {
        let report = WorkflowDoctorReport {
            schema_version: WORKFLOW_DOCTOR_SCHEMA_VERSION,
            file: "tasks/example.md".to_string(),
            outcome: WorkflowDoctorOutcome::Recoverable,
            catalog_contract_version: "test".to_string(),
            facts: WorkflowDoctorFacts::default(),
            warnings: vec!["heads up".to_string()],
            results: vec![WorkflowInvariantResult {
                id: WorkflowInvariantId::CloseoutCommit,
                title: "Closeout commit".to_string(),
                outcome: WorkflowDoctorOutcome::Recoverable,
                evidence: vec!["captured response".to_string()],
                missing_fact_sources: vec!["session_check.ok".to_string()],
                disproof_markers: vec!["stale marker".to_string()],
                repair_commands: vec!["agent-doc write --commit tasks/example.md".to_string()],
                operator_actions: vec!["operator review".to_string()],
            }],
        };

        let text = format_text_report(&report);

        assert!(text.contains("workflow doctor: tasks/example.md outcome=recoverable\n"));
        assert!(text.contains("warning: heads up\n"));
        assert!(text.contains("- closeout_commit outcome=recoverable title=Closeout commit\n"));
        assert!(text.contains("  evidence: captured response\n"));
        assert!(text.contains("  missing: session_check.ok\n"));
        assert!(text.contains("  disproof: stale marker\n"));
        assert!(text.contains("  repair: agent-doc write --commit tasks/example.md\n"));
        assert!(text.contains("  operator: operator review\n"));
    }
}
