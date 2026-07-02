//! Invariant-driven autofix planning.
//!
//! The planner consumes the workflow doctor report and the invariant catalog. It
//! records one append-only proof marker per invariant/root-cause fingerprint so
//! repeated symptoms can de-duplicate before they become duplicate queue or
//! backlog work. `--apply` executes only the small v1 whitelist of repairs whose
//! safety proof is available in the doctor result.

use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::doctor::{WorkflowDoctorOptions, diagnose};
use agent_doc_workflow::autofix::{
    WORKFLOW_AUTOFIX_SCHEMA_VERSION, WorkflowAutofixReport, WorkflowAutofixStep,
    WorkflowAutofixStepStatus, plan_autofix_steps,
};
use agent_doc_workflow::invariants::workflow_invariant_catalog;
use agent_doc_workflow_io::proof_ledger::{
    OperationProofInput, OperationProofRecord, ProofEvidenceKind, ProofOperationKind, ProofOutcome,
    append_operation_proof, proof_ledger_path, read_operation_proofs,
};

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
