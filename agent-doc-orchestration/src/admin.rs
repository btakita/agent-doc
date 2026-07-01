//! Fleet-wide operational control plane (`#ipc-admin-api`).
//!
//! The initial read-only verbs replaced the manual `ps` / `pstree` / `pgrep`
//! forensics required to resolve a cross-document actor contamination incident
//! (`#xdoc-route-sweep-commits-sibling-docs`):
//!
//! - `agent-doc admin list` — enumerate every actor in the project fleet (one
//!   row per document: session, pane, window, supervisor pid, harness,
//!   generation, state, and pane liveness).
//! - `agent-doc admin detect` — derived diagnostics over the actor store: a live
//!   pane that is the authoritative binding of more than one non-closed document
//!   (the cross-document execution contamination), and non-closed actors whose
//!   pane is dead (orphaned bindings that route/sync should reap).
//!
//! Controller-backed mutating verbs now live here too: inspect, queue
//! pause/resume/drain, stale actor reap, generation-checked handoff, and
//! projection repair. Keeping the command logic in the binary follows the
//! Shared-Foundation rule: editor plugins shell the CLI/FFI rather than
//! re-deriving fleet state.

use anyhow::{Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::sessions;
use agent_doc_controller::fleet::{
    ActorListRecord, ActorListRegistryBinding, AdminReceiptLine, build_admin_actor_list,
    detect_admin_findings, format_admin_receipt_line,
};
use agent_doc_controller::status::controller_freshness_summary;
use tmux_router::Tmux;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReapAllStaleSummary {
    pub project_root: String,
    pub reaped: usize,
    pub kept: usize,
    pub reason: String,
}

/// Resolve the project root for fleet enumeration: explicit `--project-root`,
/// else the nearest `.agent-doc` ancestor of the current directory.
fn resolve_root(project_root: Option<&Path>) -> Result<PathBuf> {
    if let Some(root) = project_root {
        return Ok(root.to_path_buf());
    }
    let cwd = std::env::current_dir().context("failed to read current directory")?;
    agent_doc_fs::find_project_root(&cwd)
        .with_context(|| format!("no .agent-doc project root found from {}", cwd.display()))
}

fn resolve_root_for_target(
    project_root: Option<&Path>,
    document: Option<&Path>,
) -> Result<PathBuf> {
    if let Some(root) = project_root {
        return Ok(root.to_path_buf());
    }
    if let Some(document) = document
        && let Some(root) = agent_doc_fs::find_project_root(document)
    {
        return Ok(root);
    }
    resolve_root(None)
}

fn print_receipt(
    receipt: &crate::project_controller::ControllerAdminReceipt,
    json: bool,
) -> Result<()> {
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

/// `agent-doc admin list` — enumerate the project fleet.
pub fn list(project_root: Option<&Path>, json: bool) -> Result<()> {
    let root = resolve_root(project_root)?;
    let actors = crate::project_controller::load_actor_store(&root)?;
    let registry = sessions::load_in(&root)?;
    let tmux = Tmux::default_server();
    let rows = build_admin_actor_list(
        actors.values().map(|record| ActorListRecord {
            document_id: record.document_id.clone(),
            session_id: record.session_id.clone(),
            pane: record.pane_id.clone(),
            window: record.window_id.clone(),
            harness: record.harness.clone(),
            generation: record.generation,
            state: record.state.as_str().to_string(),
        }),
        registry.values().map(|entry| ActorListRegistryBinding {
            session_id: entry.session_id.clone(),
            supervisor_pid: entry.pid,
            cwd: entry.cwd.clone(),
        }),
        |pane| tmux.pane_alive(pane),
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

/// `agent-doc admin detect` — derived fleet diagnostics.
pub fn detect(project_root: Option<&Path>, json: bool) -> Result<()> {
    let root = resolve_root(project_root)?;
    let actors = crate::project_controller::load_actor_store(&root)?;
    let registry = sessions::load_in(&root)?;
    let tmux = Tmux::default_server();
    let rows = build_admin_actor_list(
        actors.values().map(|record| ActorListRecord {
            document_id: record.document_id.clone(),
            session_id: record.session_id.clone(),
            pane: record.pane_id.clone(),
            window: record.window_id.clone(),
            harness: record.harness.clone(),
            generation: record.generation,
            state: record.state.as_str().to_string(),
        }),
        registry.values().map(|entry| ActorListRegistryBinding {
            session_id: entry.session_id.clone(),
            supervisor_pid: entry.pid,
            cwd: entry.cwd.clone(),
        }),
        |pane| tmux.pane_alive(pane),
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
    for f in &findings {
        println!("  [{}] {}", f.kind, f.detail);
    }
    Ok(())
}

/// `agent-doc admin inspect` — inspect one actor plus controller receipts.
pub fn inspect(
    project_root: Option<&Path>,
    document: Option<&Path>,
    session: Option<&str>,
    pane: Option<&str>,
    json: bool,
) -> Result<()> {
    let root = resolve_root_for_target(project_root, document)?;
    let inspection = crate::project_controller::inspect_actor(&root, document, session, pane)?;
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
    project_root: Option<&Path>,
    document: Option<&Path>,
    action: &str,
    observed_generation: Option<u64>,
    reason: Option<&str>,
    item_id: Option<&str>,
    json: bool,
) -> Result<()> {
    let root = resolve_root_for_target(project_root, document)?;
    let receipt = crate::project_controller::control_queue(
        &root,
        document,
        action,
        observed_generation,
        reason,
        item_id,
    )?;
    print_receipt(&receipt, json)
}

pub fn reap(
    project_root: Option<&Path>,
    document: Option<&Path>,
    session: Option<&str>,
    pane: Option<&str>,
    observed_generation: u64,
    reason: &str,
    json: bool,
) -> Result<()> {
    let root = resolve_root_for_target(project_root, document)?;
    let receipt = crate::project_controller::admin_reap(
        &root,
        document,
        session,
        pane,
        observed_generation,
        reason,
    )?;
    print_receipt(&receipt, json)
}

pub fn reap_all_stale_with_liveness(
    root: &Path,
    pane_alive: impl FnMut(&str) -> bool,
    reason: &str,
) -> Result<ReapAllStaleSummary> {
    let stored_reason = format!("manual_reap_all_stale {reason}");
    let (reaped, kept) = crate::project_controller::close_stale_dead_pane_actors_for_caller(
        root,
        pane_alive,
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

pub fn reap_all_stale(project_root: Option<&Path>, reason: &str, json: bool) -> Result<()> {
    let root = resolve_root(project_root)?;
    let stored_reason = format!("manual_reap_all_stale {reason}");
    let (reaped, kept) =
        crate::project_controller::close_stale_dead_pane_actors_with_tmux_for_caller(
            &root,
            false,
            "admin",
            &stored_reason,
        )?;
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
    project_root: Option<&Path>,
    document: &Path,
    to_pane: &str,
    observed_generation: u64,
    reason: &str,
    json: bool,
) -> Result<()> {
    let root = resolve_root_for_target(project_root, Some(document))?;
    let receipt = crate::project_controller::admin_handoff(
        &root,
        document,
        to_pane,
        observed_generation,
        reason,
    )?;
    print_receipt(&receipt, json)
}

pub fn repair_projection(
    project_root: Option<&Path>,
    document: Option<&Path>,
    projection: &str,
    observed_generation: Option<u64>,
    reason: Option<&str>,
    json: bool,
) -> Result<()> {
    let root = resolve_root_for_target(project_root, document)?;
    let receipt = crate::project_controller::repair_projection(
        &root,
        document,
        projection,
        observed_generation,
        reason,
    )?;
    print_receipt(&receipt, json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_sqlite::state_store::ActorState;
    use agent_doc_sqlite::state_store::{ActorLastTransition, ActorRecord};

    fn record(document_id: &str, session_id: &str, pane: &str, state: ActorState) -> ActorRecord {
        ActorRecord {
            document_id: document_id.to_string(),
            session_id: session_id.to_string(),
            generation: 1,
            pane_id: pane.to_string(),
            window_id: "@1".to_string(),
            harness: "codex".to_string(),
            state,
            last_transition: ActorLastTransition {
                caller: "start".to_string(),
                reason: "session_start".to_string(),
                timestamp: 10,
                prior_generation: 0,
                new_generation: 1,
            },
        }
    }

    #[test]
    fn reap_all_stale_with_liveness_closes_detected_dead_pane_actors() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let dead_doc = dir.path().join("tasks/a.md");
        let live_doc = dir.path().join("tasks/b.md");
        std::fs::create_dir_all(dead_doc.parent().unwrap()).unwrap();
        std::fs::write(&dead_doc, "body").unwrap();
        std::fs::write(&live_doc, "body").unwrap();
        let dead_id = dead_doc.to_string_lossy().to_string();
        let live_id = live_doc.to_string_lossy().to_string();
        crate::project_controller::store_actor_record(
            dir.path(),
            Some(0),
            &record(&dead_id, "sid-dead", "%dead", ActorState::Ready),
        )
        .unwrap();
        crate::project_controller::store_actor_record(
            dir.path(),
            Some(0),
            &record(&live_id, "sid-live", "%live", ActorState::Busy),
        )
        .unwrap();

        let summary =
            reap_all_stale_with_liveness(dir.path(), |pane| pane == "%live", "test bulk").unwrap();
        assert_eq!(summary.reaped, 1);
        assert_eq!(summary.kept, 1);
        assert_eq!(summary.reason, "manual_reap_all_stale test bulk");

        let dead = crate::project_controller::load_actor_record(dir.path(), &dead_id)
            .unwrap()
            .unwrap();
        assert_eq!(dead.state, ActorState::Closed);
        assert_eq!(dead.pane_id, "");
        assert_eq!(
            dead.last_transition.reason,
            "manual_reap_all_stale test bulk"
        );
        let live = crate::project_controller::load_actor_record(dir.path(), &live_id)
            .unwrap()
            .unwrap();
        assert_eq!(live.state, ActorState::Busy);
        assert_eq!(live.pane_id, "%live");
    }
}
