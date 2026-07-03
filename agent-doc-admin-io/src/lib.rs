//! Admin command IO host for controller/fleet operations.

use agent_doc_controller::fleet::{
    ActorListRecord, ActorListRegistryBinding, AdminReceiptLine, build_admin_actor_list,
    detect_admin_findings, format_admin_receipt_line,
};
use agent_doc_controller::status::{ControllerFreshnessStatus, controller_freshness_summary};
use agent_doc_sqlite::state_store::{
    ActorRecord, AdminOperationStatus, DispatchAttemptStatus, ProjectionDiagnosticStatus,
    QueueBackpressureStatus, QueueControlStatus, QueueHeadStatus, SupervisorLeaseStatus,
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerAdminReceiptView {
    pub receipt_id: u64,
    pub operation_kind: String,
    #[serde(default)]
    pub document_id: Option<String>,
    pub status: String,
    #[serde(default)]
    pub diagnostic_payload: Option<String>,
    #[serde(default)]
    pub failed_stage: Option<String>,
    #[serde(default)]
    pub unblock_hint: Option<String>,
    #[serde(default)]
    pub observed_generation: Option<u64>,
    #[serde(default)]
    pub current_generation: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerActorInspectionView {
    pub target: String,
    #[serde(default)]
    pub document_id: Option<String>,
    #[serde(default)]
    pub record: Option<ActorRecord>,
    #[serde(default)]
    pub supervisor_lease: Option<SupervisorLeaseStatus>,
    #[serde(default)]
    pub freshness: Option<ControllerFreshnessStatus>,
    #[serde(default)]
    pub queue_head: Option<QueueHeadStatus>,
    #[serde(default)]
    pub queue_control: Option<QueueControlStatus>,
    #[serde(default)]
    pub queue_backpressure: Vec<QueueBackpressureStatus>,
    pub projection_lag: bool,
    pub dispatch_attempts: Vec<DispatchAttemptStatus>,
    pub admin_operations: Vec<AdminOperationStatus>,
    pub projection_diagnostics: Vec<ProjectionDiagnosticStatus>,
}

pub trait AdminControllerEffects {
    fn load_actor_list(&self, root: &Path) -> Result<Vec<ActorListRecord>>;
    fn load_registry_bindings(&self, root: &Path) -> Result<Vec<ActorListRegistryBinding>>;
    fn pane_alive(&self, pane: &str) -> bool;

    fn inspect_actor(
        &self,
        root: &Path,
        document: Option<&Path>,
        session: Option<&str>,
        pane: Option<&str>,
    ) -> Result<ControllerActorInspectionView>;

    fn control_queue(
        &self,
        root: &Path,
        document: Option<&Path>,
        action: &str,
        observed_generation: Option<u64>,
        reason: Option<&str>,
        item_id: Option<&str>,
    ) -> Result<ControllerAdminReceiptView>;

    fn admin_reap(
        &self,
        root: &Path,
        document: Option<&Path>,
        session: Option<&str>,
        pane: Option<&str>,
        observed_generation: u64,
        reason: &str,
    ) -> Result<ControllerAdminReceiptView>;

    fn close_stale_dead_pane_actors_with_liveness(
        &self,
        root: &Path,
        pane_alive: &mut dyn FnMut(&str) -> bool,
        dry_run: bool,
        caller: &str,
        reason: &str,
    ) -> Result<(usize, usize)>;

    fn close_stale_dead_pane_actors_with_tmux(
        &self,
        root: &Path,
        dry_run: bool,
        caller: &str,
        reason: &str,
    ) -> Result<(usize, usize)>;

    fn admin_handoff(
        &self,
        root: &Path,
        document: &Path,
        to_pane: &str,
        observed_generation: u64,
        reason: &str,
    ) -> Result<ControllerAdminReceiptView>;

    fn repair_projection(
        &self,
        root: &Path,
        document: Option<&Path>,
        projection: &str,
        observed_generation: Option<u64>,
        reason: Option<&str>,
    ) -> Result<ControllerAdminReceiptView>;
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReapAllStaleSummary {
    pub project_root: String,
    pub reaped: usize,
    pub kept: usize,
    pub reason: String,
}

#[derive(Debug, Clone, Copy)]
pub struct QueueControlOptions<'a> {
    pub project_root: Option<&'a Path>,
    pub document: Option<&'a Path>,
    pub action: &'a str,
    pub observed_generation: Option<u64>,
    pub reason: Option<&'a str>,
    pub item_id: Option<&'a str>,
    pub json: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct ReapOptions<'a> {
    pub project_root: Option<&'a Path>,
    pub document: Option<&'a Path>,
    pub session: Option<&'a str>,
    pub pane: Option<&'a str>,
    pub observed_generation: u64,
    pub reason: &'a str,
    pub json: bool,
}

fn print_receipt(receipt: &ControllerAdminReceiptView, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(receipt)?);
        return Ok(());
    }
    let line = format_admin_receipt_line(AdminReceiptLine {
        operation_kind: receipt.operation_kind.as_str(),
        status: receipt.status.as_str(),
        receipt_id: receipt.receipt_id,
        document_id: receipt.document_id.as_deref(),
        failed_stage: receipt.failed_stage.as_deref(),
        current_generation: receipt.current_generation,
        unblock_hint: receipt.unblock_hint.as_deref(),
    });
    println!("{line}");
    Ok(())
}

pub fn list(
    effects: &impl AdminControllerEffects,
    project_root: Option<&Path>,
    json: bool,
) -> Result<()> {
    let root = agent_doc_project_root_io::project_root_or_cwd(project_root)?;
    let rows = build_admin_actor_list(
        effects.load_actor_list(&root)?,
        effects.load_registry_bindings(&root)?,
        |pane| effects.pane_alive(pane),
    );

    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    if rows.is_empty() {
        println!("No registered actors in {}", root.display());
        return Ok(());
    }
    println!("{} actor(s) in {}:", rows.len(), root.display());
    for row in &rows {
        println!(
            "  {} [{}] pane={} ({}) gen={} {} pid={} session={}",
            row.document_id,
            row.harness,
            row.pane,
            if row.pane_alive { "alive" } else { "dead" },
            row.generation,
            row.state,
            row.supervisor_pid
                .map(|p| p.to_string())
                .unwrap_or_else(|| "?".to_string()),
            row.session_id,
        );
    }
    Ok(())
}

pub fn detect(
    effects: &impl AdminControllerEffects,
    project_root: Option<&Path>,
    json: bool,
) -> Result<()> {
    let root = agent_doc_project_root_io::project_root_or_cwd(project_root)?;
    let rows = build_admin_actor_list(
        effects.load_actor_list(&root)?,
        effects.load_registry_bindings(&root)?,
        |pane| effects.pane_alive(pane),
    );
    let findings = detect_admin_findings(&rows);

    if json {
        println!("{}", serde_json::to_string_pretty(&findings)?);
        return Ok(());
    }
    if findings.is_empty() {
        println!("No fleet anomalies detected in {}", root.display());
        return Ok(());
    }
    println!("{} finding(s) in {}:", findings.len(), root.display());
    for finding in &findings {
        println!("  [{}] {}", finding.kind, finding.detail);
    }
    Ok(())
}

pub fn inspect(
    effects: &impl AdminControllerEffects,
    project_root: Option<&Path>,
    document: Option<&Path>,
    session: Option<&str>,
    pane: Option<&str>,
    json: bool,
) -> Result<()> {
    let root = agent_doc_project_root_io::project_root_for_target_or_cwd(project_root, document)?;
    let inspection = effects.inspect_actor(&root, document, session, pane)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&inspection)?);
    } else if let Some(record) = inspection.record.as_ref() {
        println!(
            "{} [{}] pane={} gen={} state={} queue_control={} projection_lag={} freshness={}",
            inspection
                .document_id
                .as_deref()
                .unwrap_or(record.document_id.as_str()),
            record.harness,
            record.pane_id,
            record.generation,
            record.state.as_str(),
            inspection
                .queue_control
                .as_ref()
                .map(|control| control.state.as_str())
                .unwrap_or("none"),
            inspection.projection_lag,
            controller_freshness_summary(inspection.freshness.as_ref())
        );
    } else {
        println!("No actor found for {}", inspection.target);
    }
    Ok(())
}

pub fn queue_control(
    effects: &impl AdminControllerEffects,
    options: QueueControlOptions<'_>,
) -> Result<()> {
    let root = agent_doc_project_root_io::project_root_for_target_or_cwd(
        options.project_root,
        options.document,
    )?;
    let receipt = effects.control_queue(
        &root,
        options.document,
        options.action,
        options.observed_generation,
        options.reason,
        options.item_id,
    )?;
    print_receipt(&receipt, options.json)
}

pub fn reap(effects: &impl AdminControllerEffects, options: ReapOptions<'_>) -> Result<()> {
    let root = agent_doc_project_root_io::project_root_for_target_or_cwd(
        options.project_root,
        options.document,
    )?;
    let receipt = effects.admin_reap(
        &root,
        options.document,
        options.session,
        options.pane,
        options.observed_generation,
        options.reason,
    )?;
    print_receipt(&receipt, options.json)
}

pub fn reap_all_stale_with_liveness(
    effects: &impl AdminControllerEffects,
    root: &Path,
    pane_alive: impl FnMut(&str) -> bool,
    reason: &str,
) -> Result<ReapAllStaleSummary> {
    let stored_reason = format!("manual_reap_all_stale {reason}");
    let mut pane_alive = pane_alive;
    let (reaped, kept) = effects.close_stale_dead_pane_actors_with_liveness(
        root,
        &mut pane_alive,
        false,
        "admin",
        &stored_reason,
    )?;
    Ok(ReapAllStaleSummary {
        project_root: root.display().to_string(),
        reaped,
        kept,
        reason: stored_reason,
    })
}

pub fn reap_all_stale(
    effects: &impl AdminControllerEffects,
    project_root: Option<&Path>,
    reason: &str,
    json: bool,
) -> Result<()> {
    let root = agent_doc_project_root_io::project_root_or_cwd(project_root)?;
    let stored_reason = format!("manual_reap_all_stale {reason}");
    let (reaped, kept) =
        effects.close_stale_dead_pane_actors_with_tmux(&root, false, "admin", &stored_reason)?;
    let summary = ReapAllStaleSummary {
        project_root: root.display().to_string(),
        reaped,
        kept,
        reason: stored_reason,
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        println!(
            "admin_reap_all_stale accepted project_root={} reaped={} kept={} reason={}",
            summary.project_root, summary.reaped, summary.kept, summary.reason
        );
    }
    Ok(())
}

pub fn handoff(
    effects: &impl AdminControllerEffects,
    project_root: Option<&Path>,
    document: &Path,
    to_pane: &str,
    observed_generation: u64,
    reason: &str,
    json: bool,
) -> Result<()> {
    let root =
        agent_doc_project_root_io::project_root_for_target_or_cwd(project_root, Some(document))?;
    let receipt = effects.admin_handoff(&root, document, to_pane, observed_generation, reason)?;
    print_receipt(&receipt, json)
}

pub fn repair_projection(
    effects: &impl AdminControllerEffects,
    project_root: Option<&Path>,
    document: Option<&Path>,
    projection: &str,
    observed_generation: Option<u64>,
    reason: Option<&str>,
    json: bool,
) -> Result<()> {
    let root = agent_doc_project_root_io::project_root_for_target_or_cwd(project_root, document)?;
    let receipt =
        effects.repair_projection(&root, document, projection, observed_generation, reason)?;
    print_receipt(&receipt, json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct FakeEffects {
        close_calls: RefCell<Vec<String>>,
    }

    impl AdminControllerEffects for FakeEffects {
        fn load_actor_list(&self, _root: &Path) -> Result<Vec<ActorListRecord>> {
            Ok(Vec::new())
        }

        fn load_registry_bindings(&self, _root: &Path) -> Result<Vec<ActorListRegistryBinding>> {
            Ok(Vec::new())
        }

        fn pane_alive(&self, _pane: &str) -> bool {
            false
        }

        fn inspect_actor(
            &self,
            _root: &Path,
            _document: Option<&Path>,
            _session: Option<&str>,
            _pane: Option<&str>,
        ) -> Result<ControllerActorInspectionView> {
            anyhow::bail!("unused")
        }

        fn control_queue(
            &self,
            _root: &Path,
            _document: Option<&Path>,
            _action: &str,
            _observed_generation: Option<u64>,
            _reason: Option<&str>,
            _item_id: Option<&str>,
        ) -> Result<ControllerAdminReceiptView> {
            anyhow::bail!("unused")
        }

        fn admin_reap(
            &self,
            _root: &Path,
            _document: Option<&Path>,
            _session: Option<&str>,
            _pane: Option<&str>,
            _observed_generation: u64,
            _reason: &str,
        ) -> Result<ControllerAdminReceiptView> {
            anyhow::bail!("unused")
        }

        fn close_stale_dead_pane_actors_with_liveness(
            &self,
            _root: &Path,
            pane_alive: &mut dyn FnMut(&str) -> bool,
            dry_run: bool,
            caller: &str,
            reason: &str,
        ) -> Result<(usize, usize)> {
            self.close_calls.borrow_mut().push(format!(
                "{dry_run}:{caller}:{reason}:{}",
                pane_alive("%dead")
            ));
            Ok((1, 2))
        }

        fn close_stale_dead_pane_actors_with_tmux(
            &self,
            _root: &Path,
            _dry_run: bool,
            _caller: &str,
            _reason: &str,
        ) -> Result<(usize, usize)> {
            anyhow::bail!("unused")
        }

        fn admin_handoff(
            &self,
            _root: &Path,
            _document: &Path,
            _to_pane: &str,
            _observed_generation: u64,
            _reason: &str,
        ) -> Result<ControllerAdminReceiptView> {
            anyhow::bail!("unused")
        }

        fn repair_projection(
            &self,
            _root: &Path,
            _document: Option<&Path>,
            _projection: &str,
            _observed_generation: Option<u64>,
            _reason: Option<&str>,
        ) -> Result<ControllerAdminReceiptView> {
            anyhow::bail!("unused")
        }
    }

    #[test]
    fn reap_all_stale_with_liveness_reports_effect_summary() {
        let effects = FakeEffects {
            close_calls: RefCell::new(Vec::new()),
        };
        let dir = tempfile::TempDir::new().unwrap();

        let summary =
            reap_all_stale_with_liveness(&effects, dir.path(), |_| false, "operator").unwrap();

        assert_eq!(summary.reaped, 1);
        assert_eq!(summary.kept, 2);
        assert_eq!(summary.reason, "manual_reap_all_stale operator");
        assert_eq!(
            effects.close_calls.borrow().as_slice(),
            ["false:admin:manual_reap_all_stale operator:false"]
        );
    }
}
