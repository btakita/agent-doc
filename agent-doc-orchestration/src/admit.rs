//! Lightweight realtime admission for response-bound turns.
//!
//! `preflight` still owns legacy maintenance, repair, and prompt classification.
//! This module is the narrow admission path used by realtime cutover work: accept
//! a document into a durable response cycle without running the heavy preflight
//! maintenance bundle.

use anyhow::{Context, Result};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdmitOutput {
    pub admitted: bool,
    pub file: String,
    pub cycle_id: String,
    pub cycle_phase: String,
    pub last_event: String,
    pub source: String,
    pub maintenance_required: bool,
    pub preflight_required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_hash: Option<String>,
}

pub fn run(file: &Path) -> Result<()> {
    let output = admit(file)?;
    let json = serde_json::to_string_pretty(&output).context("failed to serialize admit output")?;
    println!("{json}");
    Ok(())
}

pub fn admit(file: &Path) -> Result<AdmitOutput> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    let disk = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let current = crate::realtime_model::resolve_current_doc(file, &disk).content;
    let snapshot = agent_doc_snapshot_io::load(file)
        .with_context(|| format!("failed to load snapshot for {}", file.display()))?;

    let state =
        agent_doc_cycle_state_io::start_preflight(file, snapshot.as_deref(), Some(&current))?;
    let phase = state.phase.as_str().to_string();
    crate::ops_log::log_op(
        file,
        &format!(
            "realtime_admit file={} cycle_id={} phase={} source=admit action=accepted maintenance_required=false preflight_required=false",
            file.display(),
            state.cycle_id,
            phase
        ),
    );

    Ok(AdmitOutput {
        admitted: true,
        file: file
            .canonicalize()
            .unwrap_or_else(|_| file.to_path_buf())
            .display()
            .to_string(),
        cycle_id: state.cycle_id,
        cycle_phase: phase,
        last_event: state.last_event,
        source: "admit".to_string(),
        maintenance_required: false,
        preflight_required: false,
        snapshot_hash: state.snapshot_hash,
        file_hash: state.file_hash,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_turn::CyclePhase;
    use std::fs;

    #[test]
    fn admit_opens_cycle_without_preflight_maintenance() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        let file = root.join("session.md");
        let original = "---\nagent_doc_session: sid-1\n---\n\n# Session\n\nOperator prompt.\n";
        fs::write(&file, original).unwrap();

        let output = admit(&file).unwrap();

        assert!(output.admitted);
        assert_eq!(output.source, "admit");
        assert!(!output.maintenance_required);
        assert!(!output.preflight_required);
        assert_eq!(output.cycle_phase, "preflight_started");
        assert_eq!(fs::read_to_string(&file).unwrap(), original);

        let state = agent_doc_cycle_state_io::load(&file).unwrap().unwrap();
        assert_eq!(state.cycle_id, output.cycle_id);
        assert_eq!(state.phase, CyclePhase::PreflightStarted);
        assert_eq!(state.last_event, "preflight_started");

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("realtime_admit"), "ops log:\n{log}");
        assert!(
            log.contains("maintenance_required=false"),
            "ops log:\n{log}"
        );
        assert!(log.contains("preflight_required=false"), "ops log:\n{log}");
        assert!(
            !log.contains("preflight_diff_start"),
            "admit must not run preflight diff start:\n{log}"
        );
        assert!(
            !log.contains("deprecated_queue_active_line_dropped"),
            "admit must not run queue maintenance:\n{log}"
        );
        assert!(
            !log.contains("layout repair"),
            "admit must not run preflight layout repair:\n{log}"
        );
    }
}
