//! Invariant-driven autofix planning.
//!
//! The planner consumes the workflow doctor report and the invariant catalog. It
//! records one append-only proof marker per invariant/root-cause fingerprint so
//! repeated symptoms can de-duplicate before they become duplicate queue or
//! backlog work. `--apply` executes only the small v1 whitelist of repairs whose
//! safety proof is available in the doctor result.
//!
//! The command exits with the status of the outcome that remains after
//! plan/apply, using the doctor's mapping ([`WorkflowDoctorOutcome::exit_code`]):
//! a plan that ran nothing reports the doctor outcome as it stands, and an
//! `--apply` that ran a repair command diagnoses again.

use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::doctor::{WorkflowDoctorEffects, WorkflowDoctorOptions, diagnose, outcome_result};
use crate::proof_ledger::{
    OperationProofInput, OperationProofRecord, ProofEvidenceKind, ProofOperationKind, ProofOutcome,
    append_operation_proof, proof_ledger_path, read_operation_proofs,
};
use agent_doc_workflow::autofix::{
    WORKFLOW_AUTOFIX_SCHEMA_VERSION, WorkflowAutofixReport, WorkflowAutofixStep,
    WorkflowAutofixStepStatus, autofix_ran_repairs, plan_autofix_steps,
};
use agent_doc_workflow::doctor::WorkflowDoctorOutcome;
use agent_doc_workflow::invariants::workflow_invariant_catalog;

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

/// Print the report, then report the outcome that remains after plan/apply as
/// the result, exactly as `agent-doc doctor` does: `Ok(())` only for `ok`,
/// otherwise a [`crate::doctor::WorkflowDoctorUnhealthy`] error whose exit
/// status is [`WorkflowDoctorOutcome::exit_code`].
pub fn run(
    file: &Path,
    options: WorkflowAutofixOptions,
    effects: &mut impl WorkflowDoctorEffects,
) -> Result<()> {
    let report = autofix(file, &options, effects)?;
    if options.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_text_report(&report);
    }
    outcome_result(report.outcome)
}

pub fn autofix(
    file: &Path,
    options: &WorkflowAutofixOptions,
    effects: &mut impl WorkflowDoctorEffects,
) -> Result<WorkflowAutofixReport> {
    let doctor_options = WorkflowDoctorOptions {
        preflight_json: options.preflight_json.clone(),
        session_check_json: options.session_check_json.clone(),
        ops_limit: options.ops_limit,
        json: false,
    };
    let doctor = diagnose(file, &doctor_options, effects)?;
    let catalog = workflow_invariant_catalog();
    let canonical = file
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", file.display()))?;
    let project_root = agent_doc_project_root_io::project_root_containing(&canonical);
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
    let outcome = remaining_outcome(doctor.outcome, &steps, || {
        diagnose(file, &doctor_options, effects)
            .map(|report| report.outcome)
            .context("diagnose the workflow again after applying autofix repairs")
    })?;
    Ok(WorkflowAutofixReport {
        schema_version: WORKFLOW_AUTOFIX_SCHEMA_VERSION,
        file: canonical.display().to_string(),
        apply: options.apply,
        dry_run: options.dry_run,
        doctor_outcome: doctor.outcome,
        outcome,
        steps,
        warnings,
    })
}

/// The outcome that remains after plan/apply: the diagnosed outcome when no
/// repair command ran, otherwise a fresh diagnosis of the repaired workflow.
/// The fresh diagnosis re-gathers live facts but reuses the supplied
/// `--preflight-json` / `--session-check-json` captures, exactly as given.
fn remaining_outcome(
    diagnosed: WorkflowDoctorOutcome,
    steps: &[WorkflowAutofixStep],
    rediagnose: impl FnOnce() -> Result<WorkflowDoctorOutcome>,
) -> Result<WorkflowDoctorOutcome> {
    if autofix_ran_repairs(steps) {
        rediagnose()
    } else {
        Ok(diagnosed)
    }
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

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

fn print_text_report(report: &WorkflowAutofixReport) {
    println!(
        "workflow autofix: {} outcome={} doctor_outcome={} apply={} dry_run={}",
        report.file,
        report.outcome.as_str(),
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
    use crate::doctor::tests::{
        DivergedEditorEffects, FRESH_DOCUMENT, FreshProjectEffects, QUEUED_DOCUMENT, fresh_project,
    };
    use crate::doctor::workflow_doctor_exit_status;
    use agent_doc_workflow::doctor::{
        WORKFLOW_DOCTOR_EXIT_BLOCKED, WORKFLOW_DOCTOR_EXIT_OPERATOR,
        WORKFLOW_DOCTOR_EXIT_RECOVERABLE,
    };
    use agent_doc_workflow::invariants::WorkflowInvariantId;

    fn json_options() -> WorkflowAutofixOptions {
        WorkflowAutofixOptions {
            json: true,
            ..WorkflowAutofixOptions::default()
        }
    }

    /// `run` resolves to the exit status the CLI returns.
    fn run_exit_status(
        file: &Path,
        options: WorkflowAutofixOptions,
        effects: &mut impl WorkflowDoctorEffects,
    ) -> u8 {
        match run(file, options, effects) {
            Ok(()) => 0,
            Err(err) => workflow_doctor_exit_status(&err)
                .unwrap_or_else(|| panic!("a completed autofix is not a failure: {err:#}")),
        }
    }

    fn step_with(status: WorkflowAutofixStepStatus) -> WorkflowAutofixStep {
        WorkflowAutofixStep {
            invariant_id: WorkflowInvariantId::StaleSupervisor,
            action: "restart_supervisor_once".to_string(),
            status,
            command: Some("agent-doc admin recycle --all-projects --json".to_string()),
            required_proof: Vec::new(),
            proof_marker: "workflow_autofix:stale_supervisor:test".to_string(),
            operation_id: "workflow_autofix:stale_supervisor".to_string(),
            content_hash: "test".to_string(),
            reason: String::new(),
            ledger_path: None,
            exit_code: None,
            output: None,
        }
    }

    #[test]
    fn run_exits_zero_when_the_workflow_is_healthy() {
        let (_dir, file) = fresh_project(FRESH_DOCUMENT);
        let report = autofix(&file, &json_options(), &mut FreshProjectEffects).unwrap();
        assert_eq!(report.outcome, WorkflowDoctorOutcome::Ok, "{report:?}");
        assert!(report.steps.is_empty(), "{report:?}");
        assert_eq!(
            run_exit_status(&file, json_options(), &mut FreshProjectEffects),
            0
        );
    }

    #[test]
    fn run_exits_recoverable_status_for_a_plan_that_was_not_applied() {
        let (_dir, file) = fresh_project(FRESH_DOCUMENT);
        let report = autofix(&file, &json_options(), &mut DivergedEditorEffects).unwrap();
        assert_eq!(report.doctor_outcome, WorkflowDoctorOutcome::Recoverable);
        assert_eq!(report.outcome, WorkflowDoctorOutcome::Recoverable);
        assert_eq!(
            run_exit_status(&file, json_options(), &mut DivergedEditorEffects),
            WORKFLOW_DOCTOR_EXIT_RECOVERABLE
        );
    }

    #[test]
    fn a_dry_run_plan_reports_the_outcome_as_it_stands() {
        let (_dir, file) = fresh_project(FRESH_DOCUMENT);
        let options = WorkflowAutofixOptions {
            dry_run: true,
            ..json_options()
        };
        let report = autofix(&file, &options, &mut DivergedEditorEffects).unwrap();
        assert!(
            report.steps.iter().all(|step| step.ledger_path.is_none()),
            "a dry run writes no proof markers: {report:?}"
        );
        assert_eq!(report.outcome, WorkflowDoctorOutcome::Recoverable);
        assert_eq!(
            run_exit_status(&file, options, &mut DivergedEditorEffects),
            WORKFLOW_DOCTOR_EXIT_RECOVERABLE
        );
    }

    #[test]
    fn an_apply_that_ran_no_repair_keeps_the_recoverable_status() {
        // The editor-convergence repair is catalog-safe but not in the automatic
        // execution whitelist, so `--apply` runs nothing and the workflow stays
        // recoverable: the command must not report success.
        let (_dir, file) = fresh_project(FRESH_DOCUMENT);
        let options = WorkflowAutofixOptions {
            apply: true,
            dry_run: true,
            ..json_options()
        };
        let report = autofix(&file, &options, &mut DivergedEditorEffects).unwrap();
        let step = report
            .steps
            .iter()
            .find(|step| step.invariant_id == WorkflowInvariantId::EditorConvergence)
            .expect("editor convergence step");
        assert_eq!(step.status, WorkflowAutofixStepStatus::UnsafeCommand);
        assert_eq!(report.outcome, WorkflowDoctorOutcome::Recoverable);
        assert_eq!(
            run_exit_status(&file, options, &mut DivergedEditorEffects),
            WORKFLOW_DOCTOR_EXIT_RECOVERABLE
        );
    }

    #[test]
    fn run_exits_operator_status_when_a_person_must_act() {
        let (dir, file) = fresh_project(FRESH_DOCUMENT);
        let logs = dir.path().join(".agent-doc/logs");
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::write(logs.join("ops.log"), "File Cache Conflict\n").unwrap();
        let options = WorkflowAutofixOptions {
            apply: true,
            ..json_options()
        };
        let report = autofix(&file, &options, &mut FreshProjectEffects).unwrap();
        assert_eq!(
            report.outcome,
            WorkflowDoctorOutcome::Operator,
            "{report:?}"
        );
        assert_eq!(
            run_exit_status(&file, options, &mut FreshProjectEffects),
            WORKFLOW_DOCTOR_EXIT_OPERATOR
        );
    }

    #[test]
    fn run_exits_blocked_status_when_evidence_is_missing() {
        let (_dir, file) = fresh_project(QUEUED_DOCUMENT);
        let options = WorkflowAutofixOptions {
            apply: true,
            ..json_options()
        };
        let report = autofix(&file, &options, &mut FreshProjectEffects).unwrap();
        assert_eq!(report.outcome, WorkflowDoctorOutcome::Blocked, "{report:?}");
        assert_eq!(
            run_exit_status(&file, options, &mut FreshProjectEffects),
            WORKFLOW_DOCTOR_EXIT_BLOCKED
        );
    }

    #[test]
    fn an_autofix_that_failed_to_run_carries_no_outcome_status() {
        let (dir, _file) = fresh_project(FRESH_DOCUMENT);
        let err = run(
            &dir.path().join("missing.md"),
            json_options(),
            &mut FreshProjectEffects,
        )
        .unwrap_err();
        assert_eq!(workflow_doctor_exit_status(&err), None, "{err:#}");
    }

    #[test]
    fn the_remaining_outcome_comes_from_a_fresh_diagnosis_only_after_a_repair_ran() {
        let not_run = [
            step_with(WorkflowAutofixStepStatus::Planned),
            step_with(WorkflowAutofixStepStatus::UnsafeCommand),
        ];
        let outcome = remaining_outcome(WorkflowDoctorOutcome::Recoverable, &not_run, || {
            panic!("nothing ran, so the diagnosed outcome stands")
        })
        .unwrap();
        assert_eq!(outcome, WorkflowDoctorOutcome::Recoverable);

        // A repair that succeeded is judged by what remains, not by its own exit.
        let repaired = [step_with(WorkflowAutofixStepStatus::Executed)];
        let outcome = remaining_outcome(WorkflowDoctorOutcome::Recoverable, &repaired, || {
            Ok(WorkflowDoctorOutcome::Ok)
        })
        .unwrap();
        assert_eq!(outcome, WorkflowDoctorOutcome::Ok);
        let outcome = remaining_outcome(WorkflowDoctorOutcome::Recoverable, &repaired, || {
            Ok(WorkflowDoctorOutcome::Operator)
        })
        .unwrap();
        assert_eq!(outcome, WorkflowDoctorOutcome::Operator);

        let failed = [step_with(WorkflowAutofixStepStatus::Failed)];
        let outcome = remaining_outcome(WorkflowDoctorOutcome::Recoverable, &failed, || {
            Ok(WorkflowDoctorOutcome::Recoverable)
        })
        .unwrap();
        assert_eq!(outcome, WorkflowDoctorOutcome::Recoverable);
        assert_eq!(
            outcome_result(outcome)
                .err()
                .as_ref()
                .and_then(workflow_doctor_exit_status),
            Some(WORKFLOW_DOCTOR_EXIT_RECOVERABLE)
        );

        let rediagnosis_failed =
            remaining_outcome(WorkflowDoctorOutcome::Recoverable, &failed, || {
                Err(anyhow::anyhow!("document vanished"))
            })
            .unwrap_err();
        assert_eq!(workflow_doctor_exit_status(&rediagnosis_failed), None);
    }
}
