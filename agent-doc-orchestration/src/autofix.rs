//! Invariant-driven autofix planning.
//!
//! The planner consumes the workflow doctor report and the invariant catalog. It
//! records one append-only proof marker per invariant/root-cause fingerprint so
//! repeated symptoms can de-duplicate before they become duplicate queue or
//! backlog work. `--apply` executes only the small v1 whitelist of repairs whose
//! safety proof is available in the doctor result.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::doctor::{
    WorkflowDoctorOptions, WorkflowDoctorOutcome, WorkflowDoctorReport, WorkflowInvariantResult,
    diagnose,
};
use crate::flow::proof_ledger::{
    OperationProofInput, OperationProofRecord, ProofEvidenceKind, ProofOperationKind, ProofOutcome,
    append_operation_proof, proof_ledger_path, read_operation_proofs,
};
use agent_doc_workflow::invariants::{
    RemediationAction, RemediationStep, WorkflowInvariantCatalog, WorkflowInvariantId,
    workflow_invariant_catalog,
};

pub const WORKFLOW_AUTOFIX_SCHEMA_VERSION: u8 = 1;
const DEFAULT_OPS_LIMIT: usize = 200;

#[derive(Debug, Clone)]
pub struct WorkflowAutofixOptions {
    pub preflight_json: Option<PathBuf>,
    pub session_check_json: Option<PathBuf>,
    pub ops_limit: usize,
    pub apply: bool,
    pub dry_run: bool,
    pub json: bool,
}

impl Default for WorkflowAutofixOptions {
    fn default() -> Self {
        Self {
            preflight_json: None,
            session_check_json: None,
            ops_limit: DEFAULT_OPS_LIMIT,
            apply: false,
            dry_run: false,
            json: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowAutofixReport {
    pub schema_version: u8,
    pub file: String,
    pub apply: bool,
    pub dry_run: bool,
    pub doctor_outcome: WorkflowDoctorOutcome,
    pub steps: Vec<WorkflowAutofixStep>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowAutofixStepStatus {
    Planned,
    Executed,
    SkippedDuplicate,
    OperatorGated,
    BlockedMissingFacts,
    UnsafeCommand,
    Failed,
}

impl WorkflowAutofixStepStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Executed => "executed",
            Self::SkippedDuplicate => "skipped_duplicate",
            Self::OperatorGated => "operator_gated",
            Self::BlockedMissingFacts => "blocked_missing_facts",
            Self::UnsafeCommand => "unsafe_command",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowAutofixStep {
    pub invariant_id: WorkflowInvariantId,
    pub action: String,
    pub status: WorkflowAutofixStepStatus,
    pub command: Option<String>,
    pub required_proof: Vec<String>,
    pub proof_marker: String,
    pub operation_id: String,
    pub content_hash: String,
    pub reason: String,
    pub ledger_path: Option<String>,
    pub exit_code: Option<i32>,
    pub output: Option<String>,
}

impl WorkflowAutofixStep {
    fn operation_key(&self) -> String {
        format!("{}:{}", self.operation_id, self.content_hash)
    }
}

pub fn run(file: &Path, options: WorkflowAutofixOptions) -> Result<()> {
    let report = autofix(file, &options)?;
    if options.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_text_report(&report);
    }
    Ok(())
}

pub fn autofix(file: &Path, options: &WorkflowAutofixOptions) -> Result<WorkflowAutofixReport> {
    let doctor_options = WorkflowDoctorOptions {
        preflight_json: options.preflight_json.clone(),
        session_check_json: options.session_check_json.clone(),
        ops_limit: options.ops_limit,
        json: false,
    };
    let doctor = diagnose(file, &doctor_options)?;
    let catalog = workflow_invariant_catalog();
    let canonical = file
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", file.display()))?;
    let project_root = agent_doc_fs::find_project_root(&canonical);
    let mut warnings = doctor.warnings.clone();
    let ledger_path = project_root
        .as_deref()
        .map(|root| proof_ledger_path(root, &canonical));
    let existing_keys = match ledger_path.as_deref() {
        Some(path) => read_operation_proofs(path)?
            .into_iter()
            .filter(|record| !(options.apply && record.proof.contains(" status=planned ")))
            .map(|record| record.operation_key())
            .collect(),
        None => {
            warnings.push("proof ledger unavailable: project root could not be found".to_string());
            BTreeSet::new()
        }
    };
    let mut steps = plan_autofix_steps(&doctor, &catalog, &existing_keys, &canonical);
    if options.apply {
        for step in &mut steps {
            apply_step(step, &canonical)?;
        }
    }
    if !options.dry_run
        && let Some(root) = project_root.as_deref()
    {
        for step in &mut steps {
            if step.status == WorkflowAutofixStepStatus::SkippedDuplicate {
                continue;
            }
            let record = proof_record(step)?;
            let path = append_operation_proof(root, &canonical, &record)?;
            step.ledger_path = Some(path.display().to_string());
        }
    }
    Ok(WorkflowAutofixReport {
        schema_version: WORKFLOW_AUTOFIX_SCHEMA_VERSION,
        file: canonical.display().to_string(),
        apply: options.apply,
        dry_run: options.dry_run,
        doctor_outcome: doctor.outcome,
        steps,
        warnings,
    })
}

fn plan_autofix_steps(
    doctor: &WorkflowDoctorReport,
    catalog: &WorkflowInvariantCatalog,
    existing_keys: &BTreeSet<String>,
    file: &Path,
) -> Vec<WorkflowAutofixStep> {
    let invariants: BTreeMap<_, _> = catalog
        .invariants
        .iter()
        .map(|invariant| (invariant.id, invariant))
        .collect();
    let mut steps = Vec::new();
    for result in &doctor.results {
        if result.outcome == WorkflowDoctorOutcome::Ok {
            continue;
        }
        let Some(invariant) = invariants.get(&result.id) else {
            continue;
        };
        match result.outcome {
            WorkflowDoctorOutcome::Recoverable => {
                if invariant.safe_remediation.is_empty() {
                    steps.push(build_step(
                        result,
                        None,
                        "safe_remediation_missing",
                        WorkflowAutofixStepStatus::UnsafeCommand,
                        None,
                        "recoverable invariant has no catalog safe remediation",
                        existing_keys,
                    ));
                    continue;
                }
                for remediation in &invariant.safe_remediation {
                    let command = repair_command(result, remediation, file);
                    let executable = command
                        .as_deref()
                        .is_some_and(|command| is_whitelisted_autofix(remediation.action, command));
                    let (status, reason) = if executable {
                        (
                            WorkflowAutofixStepStatus::Planned,
                            "catalog safe remediation is whitelisted for automatic execution",
                        )
                    } else {
                        (
                            WorkflowAutofixStepStatus::UnsafeCommand,
                            "catalog remediation requires operator/manual execution or a non-whitelisted command",
                        )
                    };
                    steps.push(build_step(
                        result,
                        Some(remediation),
                        action_name(remediation.action),
                        status,
                        command,
                        reason,
                        existing_keys,
                    ));
                }
            }
            WorkflowDoctorOutcome::Operator => {
                if invariant.operator_gated_remediation.is_empty() {
                    steps.push(build_step(
                        result,
                        None,
                        "operator_gated",
                        WorkflowAutofixStepStatus::OperatorGated,
                        result.operator_actions.first().cloned(),
                        "operator action required before any safe repair can run",
                        existing_keys,
                    ));
                    continue;
                }
                for remediation in &invariant.operator_gated_remediation {
                    steps.push(build_step(
                        result,
                        Some(remediation),
                        action_name(remediation.action),
                        WorkflowAutofixStepStatus::OperatorGated,
                        result.operator_actions.first().cloned(),
                        "operator-gated remediation requires explicit human proof",
                        existing_keys,
                    ));
                }
            }
            WorkflowDoctorOutcome::Blocked => {
                steps.push(build_step(
                    result,
                    None,
                    "gather_missing_facts",
                    WorkflowAutofixStepStatus::BlockedMissingFacts,
                    result.repair_commands.first().cloned(),
                    "required fact sources are missing; gather evidence before repair",
                    existing_keys,
                ));
            }
            WorkflowDoctorOutcome::Ok => {}
        }
    }
    steps
}

fn build_step(
    result: &WorkflowInvariantResult,
    remediation: Option<&RemediationStep>,
    action: &str,
    status: WorkflowAutofixStepStatus,
    command: Option<String>,
    reason: &str,
    existing_keys: &BTreeSet<String>,
) -> WorkflowAutofixStep {
    let required_proof = remediation
        .map(|step| step.required_proof.clone())
        .unwrap_or_else(|| result.missing_fact_sources.clone());
    let operation_id = format!("workflow_autofix:{}", result.id);
    let fingerprint = format!(
        "invariant={};outcome={};action={};command={:?};evidence={:?};missing={:?};disproof={:?}",
        result.id,
        result.outcome.as_str(),
        action,
        command,
        result.evidence,
        result.missing_fact_sources,
        result.disproof_markers
    );
    let content_hash = crate::ops_log::content_hash(&fingerprint);
    let mut step = WorkflowAutofixStep {
        invariant_id: result.id,
        action: action.to_string(),
        status,
        command,
        required_proof,
        proof_marker: format!("workflow_autofix:{}:{content_hash}", result.id),
        operation_id,
        content_hash,
        reason: reason.to_string(),
        ledger_path: None,
        exit_code: None,
        output: None,
    };
    if existing_keys.contains(&step.operation_key()) {
        step.status = WorkflowAutofixStepStatus::SkippedDuplicate;
        step.reason = "matching workflow_autofix proof marker already exists".to_string();
    }
    step
}

fn repair_command(
    result: &WorkflowInvariantResult,
    remediation: &RemediationStep,
    file: &Path,
) -> Option<String> {
    result
        .repair_commands
        .iter()
        .find(|command| !command.contains("<BASELINE>"))
        .cloned()
        .or_else(|| remediation.command.clone())
        .map(|command| command.replace("<FILE>", &file.display().to_string()))
}

fn apply_step(step: &mut WorkflowAutofixStep, file: &Path) -> Result<()> {
    if step.status != WorkflowAutofixStepStatus::Planned {
        return Ok(());
    }
    let Some(command) = step.command.as_deref() else {
        step.status = WorkflowAutofixStepStatus::UnsafeCommand;
        step.reason = "no executable command was produced".to_string();
        return Ok(());
    };
    let output = match step.action.as_str() {
        "restart_supervisor_once" if command == "agent-doc admin recycle --all-projects --json" => {
            Command::new("agent-doc")
                .args(["admin", "recycle", "--all-projects", "--json"])
                .output()
                .context("execute stale-supervisor recycle autofix")?
        }
        "finalize_or_write_commit"
            if command == format!("agent-doc write --commit {}", file.display()) =>
        {
            Command::new("agent-doc")
                .args(["write", "--commit"])
                .arg(file)
                .output()
                .context("execute closeout write-commit autofix")?
        }
        _ => {
            step.status = WorkflowAutofixStepStatus::UnsafeCommand;
            step.reason = "command is not in the automatic execution whitelist".to_string();
            return Ok(());
        }
    };
    step.exit_code = output.status.code();
    step.output = Some(truncate_output(&output));
    if output.status.success() {
        step.status = WorkflowAutofixStepStatus::Executed;
        step.reason = "automatic remediation command completed successfully".to_string();
    } else {
        step.status = WorkflowAutofixStepStatus::Failed;
        step.reason = "automatic remediation command exited non-zero".to_string();
    }
    Ok(())
}

fn is_whitelisted_autofix(action: RemediationAction, command: &str) -> bool {
    match action {
        RemediationAction::RestartSupervisorOnce => {
            command == "agent-doc admin recycle --all-projects --json"
        }
        RemediationAction::FinalizeOrWriteCommit => {
            command.starts_with("agent-doc write --commit ")
                && !command.contains("&&")
                && !command.contains('<')
                && !command.contains('>')
        }
        _ => false,
    }
}

fn proof_record(step: &WorkflowAutofixStep) -> Result<OperationProofRecord> {
    OperationProofRecord::new(OperationProofInput {
        operation_id: step.operation_id.clone(),
        operation_kind: ProofOperationKind::TerminalProof,
        outcome: match step.status {
            WorkflowAutofixStepStatus::OperatorGated
            | WorkflowAutofixStepStatus::BlockedMissingFacts
            | WorkflowAutofixStepStatus::UnsafeCommand => ProofOutcome::Deferred,
            WorkflowAutofixStepStatus::Failed => ProofOutcome::Retried,
            WorkflowAutofixStepStatus::Planned | WorkflowAutofixStepStatus::Executed => {
                ProofOutcome::Recorded
            }
            WorkflowAutofixStepStatus::SkippedDuplicate => ProofOutcome::Recorded,
        },
        subject_id: Some(step.invariant_id.to_string()),
        content_hash: step.content_hash.clone(),
        proof_kind: ProofEvidenceKind::TerminalStateObserved,
        proof: format!(
            "{} status={} action={} reason={} command={:?}",
            step.proof_marker,
            step.status.as_str(),
            step.action,
            step.reason,
            step.command
        ),
        recorded_at_ms: now_millis(),
    })
}

fn truncate_output(output: &std::process::Output) -> String {
    let mut text = String::new();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stdout.trim().is_empty() {
        text.push_str(stdout.trim());
    }
    if !stderr.trim().is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(stderr.trim());
    }
    const LIMIT: usize = 600;
    if text.len() > LIMIT {
        text.truncate(LIMIT);
        text.push_str("...");
    }
    text
}

fn action_name(action: RemediationAction) -> &'static str {
    match action {
        RemediationAction::ContinueQueueDrain => "continue_queue_drain",
        RemediationAction::RestartSupervisorOnce => "restart_supervisor_once",
        RemediationAction::FinalizeOrWriteCommit => "finalize_or_write_commit",
        RemediationAction::UseEditorIpcWriteback => "use_editor_ipc_writeback",
        RemediationAction::RetryOnCurrentGeneration => "retry_on_current_generation",
        RemediationAction::CommitParentGitlink => "commit_parent_gitlink",
        RemediationAction::AskOperatorLiveEditorProof => "ask_operator_live_editor_proof",
        RemediationAction::AskOperatorResolveGitState => "ask_operator_resolve_git_state",
        RemediationAction::AskOperatorResolveConflict => "ask_operator_resolve_conflict",
    }
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

fn print_text_report(report: &WorkflowAutofixReport) {
    println!(
        "workflow autofix: {} doctor_outcome={} apply={} dry_run={}",
        report.file,
        report.doctor_outcome.as_str(),
        report.apply,
        report.dry_run
    );
    for warning in &report.warnings {
        println!("warning: {warning}");
    }
    for step in &report.steps {
        println!(
            "- {} action={} status={} marker={}",
            step.invariant_id,
            step.action,
            step.status.as_str(),
            step.proof_marker
        );
        println!("  reason: {}", step.reason);
        if let Some(command) = &step.command {
            println!("  command: {command}");
        }
        if !step.required_proof.is_empty() {
            println!("  required_proof: {}", step.required_proof.join("; "));
        }
        if let Some(path) = &step.ledger_path {
            println!("  ledger: {path}");
        }
        if let Some(code) = step.exit_code {
            println!("  exit_code: {code}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doctor::{WorkflowDoctorFacts, evaluate_catalog};

    fn step_for(report: &WorkflowAutofixReport, id: WorkflowInvariantId) -> &WorkflowAutofixStep {
        report
            .steps
            .iter()
            .find(|step| step.invariant_id == id)
            .expect("autofix step")
    }

    fn report_from_facts(
        facts: WorkflowDoctorFacts,
        existing_keys: BTreeSet<String>,
    ) -> WorkflowAutofixReport {
        let catalog = workflow_invariant_catalog();
        let doctor = evaluate_catalog(
            Path::new("tasks/example.md"),
            catalog.clone(),
            facts,
            vec![],
        );
        let steps = plan_autofix_steps(
            &doctor,
            &catalog,
            &existing_keys,
            Path::new("tasks/example.md"),
        );
        WorkflowAutofixReport {
            schema_version: WORKFLOW_AUTOFIX_SCHEMA_VERSION,
            file: "tasks/example.md".to_string(),
            apply: false,
            dry_run: true,
            doctor_outcome: doctor.outcome,
            steps,
            warnings: vec![],
        }
    }

    fn quiet_ok_facts() -> WorkflowDoctorFacts {
        let mut facts = WorkflowDoctorFacts::default();
        facts.preflight.json_provided = true;
        facts.preflight.queue_active = Some(false);
        facts.session_check.ok = Some(true);
        facts.actor.inspection_available = true;
        facts.actor.generation = Some(7);
        facts.actor.controller_fresh = Some(true);
        facts.actor.supervisor_fresh = Some(true);
        facts.editor.patches_dir_present = true;
        facts
    }

    #[test]
    fn autofix_plans_whitelisted_stale_supervisor_recycle() {
        let mut facts = quiet_ok_facts();
        facts.actor.supervisor_fresh = Some(false);

        let report = report_from_facts(facts, BTreeSet::new());
        let step = step_for(&report, WorkflowInvariantId::StaleSupervisor);

        assert_eq!(step.status, WorkflowAutofixStepStatus::Planned);
        assert_eq!(step.action, "restart_supervisor_once");
        assert_eq!(
            step.command.as_deref(),
            Some("agent-doc admin recycle --all-projects --json")
        );
        assert!(
            step.proof_marker
                .starts_with("workflow_autofix:stale_supervisor:")
        );
    }

    #[test]
    fn autofix_gates_operator_editor_convergence() {
        let mut facts = quiet_ok_facts();
        facts.editor.live_buffer_diverges = Some(true);

        let report = report_from_facts(facts, BTreeSet::new());
        let step = step_for(&report, WorkflowInvariantId::EditorConvergence);

        assert_eq!(step.status, WorkflowAutofixStepStatus::OperatorGated);
        assert_eq!(step.action, "ask_operator_live_editor_proof");
    }

    #[test]
    fn autofix_dedupes_existing_invariant_marker() {
        let mut facts = quiet_ok_facts();
        facts.actor.supervisor_fresh = Some(false);

        let first = report_from_facts(facts.clone(), BTreeSet::new());
        let first_step = step_for(&first, WorkflowInvariantId::StaleSupervisor);
        let mut existing = BTreeSet::new();
        existing.insert(first_step.operation_key());

        let second = report_from_facts(facts, existing);
        let second_step = step_for(&second, WorkflowInvariantId::StaleSupervisor);

        assert_eq!(
            second_step.status,
            WorkflowAutofixStepStatus::SkippedDuplicate
        );
        assert_eq!(
            second_step.reason,
            "matching workflow_autofix proof marker already exists"
        );
    }
}
