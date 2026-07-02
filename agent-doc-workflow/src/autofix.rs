//! Pure workflow autofix planning.
//!
//! This module decides which invariant remediation steps are safe to plan and
//! how to fingerprint them. Callers own command execution, proof-ledger IO, and
//! user-facing command options.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::doctor::{WorkflowDoctorOutcome, WorkflowDoctorReport, WorkflowInvariantResult};
use crate::invariants::{RemediationStep, WorkflowInvariantCatalog, WorkflowInvariantId};

pub const WORKFLOW_AUTOFIX_SCHEMA_VERSION: u8 = 1;

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
    pub const fn as_str(self) -> &'static str {
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
    pub fn operation_key(&self) -> String {
        format!("{}:{}", self.operation_id, self.content_hash)
    }
}

pub fn plan_autofix_steps(
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
                        .is_some_and(|command| remediation.action.accepts_autofix_command(command));
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
                        remediation.action.as_str(),
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
                        remediation.action.as_str(),
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
    let content_hash = agent_doc_hash::content_hash(&fingerprint);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doctor::{WorkflowDoctorFacts, evaluate_catalog};
    use crate::invariants::workflow_invariant_catalog;

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
