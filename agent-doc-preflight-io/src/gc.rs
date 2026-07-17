//! Preflight automatic GC and stale actor cleanup adapters.

use anyhow::Result;
use std::path::Path;
use std::time::Duration;

const PREFLIGHT_AUTO_GC_SCOPE: &str = "preflight_auto_gc";
const PROJECT_SCOPE_ID: &str = "project";

struct PreflightGcControllerEffects;

impl agent_doc_gc_io::GcControllerEffects for PreflightGcControllerEffects {
    fn close_stale_starting_actors(
        &mut self,
        project_root: &Path,
        stale_after: Duration,
        dry_run: bool,
    ) -> Result<(usize, usize)> {
        agent_doc_controller_io::project_controller::close_stale_starting_actors(
            project_root,
            stale_after,
            dry_run,
        )
    }

    fn close_stale_dead_pane_actors(
        &mut self,
        project_root: &Path,
        dry_run: bool,
        caller: &str,
        reason: &str,
    ) -> Result<(usize, usize)> {
        agent_doc_controller_io::project_controller::close_stale_dead_pane_actors_with_tmux_for_caller(
            project_root,
            dry_run,
            caller,
            reason,
        )
    }

    fn prune_dead_actors(
        &mut self,
        project_root: &Path,
        prune_after: Duration,
        dry_run: bool,
    ) -> Result<(usize, usize)> {
        agent_doc_controller_io::project_controller::prune_dead_actors(
            project_root,
            prune_after,
            dry_run,
        )
    }

    fn terminate_stale_preparing_controllers(
        &mut self,
        project_root: &Path,
        stale_after: Duration,
        dry_run: bool,
    ) -> Result<(usize, usize)> {
        agent_doc_controller_io::project_controller::terminate_stale_preparing_controllers(
            project_root,
            stale_after,
            dry_run,
        )
    }

    fn reap_orphaned_preparing_controllers(
        &mut self,
        project_root: &Path,
        stale_after: Duration,
        dry_run: bool,
    ) -> Result<(usize, usize)> {
        agent_doc_controller_io::project_controller::reap_orphaned_preparing_controllers(
            project_root,
            stale_after,
            dry_run,
        )
    }

    fn reap_orphaned_preparing_controllers_all_projects(
        &mut self,
        stale_after: Duration,
        dry_run: bool,
        caller: &str,
    ) -> Result<(usize, usize)> {
        agent_doc_controller_io::project_controller::reap_orphaned_preparing_controllers_all_projects(
            stale_after,
            dry_run,
            caller,
        )
    }

    fn reap_removed_project_root_controllers_all_projects(
        &mut self,
        stale_after: Duration,
        dry_run: bool,
        caller: &str,
    ) -> Result<(usize, usize)> {
        agent_doc_controller_io::project_controller::reap_removed_project_root_controllers_all_projects(
            stale_after,
            dry_run,
            caller,
        )
    }
}

fn run_auto_gc(root: &Path) -> Result<agent_doc_gc_io::GcResult> {
    let mut effects = PreflightGcControllerEffects;
    agent_doc_gc_io::run_with_controller_effects(
        Some(root),
        false,
        &mut effects,
        agent_doc_gc_io::GcControllerConfig {
            stale_starting_after: Duration::from_secs(3600),
            dead_actor_prune_after:
                agent_doc_controller_io::project_controller::DEAD_ACTOR_PRUNE_AFTER,
            stale_preparing_controller_after:
                agent_doc_controller_io::project_controller::stale_preparing_controller_threshold(),
        },
    )
}

/// Run the lightweight preflight auto-GC path.
///
/// Stale starting actors are checked every preflight. The broader GC pass is
/// gated to once per day by the project coordination state in `state.db`.
pub fn run_preflight_auto_gc(file: &Path) {
    let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let Some(root) = agent_doc_project_root_io::project_root_containing(&canonical) else {
        return;
    };
    match agent_doc_controller_io::project_controller::close_stale_starting_actors_for_caller(
        &root,
        Duration::from_secs(3600),
        false,
        "preflight",
    ) {
        Ok((closed, kept)) if closed > 0 => {
            eprintln!(
                "[preflight] actors: {} stale starting closed, {} still active",
                closed, kept
            );
        }
        Ok(_) => {}
        Err(e) => eprintln!("[preflight] actor gc warning: {}", e),
    }
    let now = current_epoch_secs();
    let needs_gc = match latest_auto_gc_at(&root) {
        Ok(Some(timestamp)) => now.saturating_sub(timestamp) > 86_400,
        Ok(None) => true,
        Err(error) => {
            eprintln!("[preflight] gc state warning: {error}");
            true
        }
    };
    if needs_gc {
        eprintln!("[preflight] step 0a: auto-gc");
        match run_auto_gc(&root) {
            Ok(result) => {
                if result.deleted > 0 {
                    eprintln!("[preflight] gc: {} files cleaned", result.deleted);
                }
                if let Err(error) = record_auto_gc_run(&root, now) {
                    eprintln!("[preflight] gc state warning: {error}");
                }
            }
            Err(e) => eprintln!("[preflight] gc warning: {}", e),
        }
    }
}

pub fn record_auto_gc_run(root: &Path, timestamp: u64) -> Result<()> {
    let canonical = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let conn = agent_doc_sqlite::state_store::open_state_db(&canonical)?;
    agent_doc_sqlite::state_store::upsert_coordination_lease_in_db(
        &conn,
        &agent_doc_sqlite::state_store::CoordinationLeaseRecord {
            scope_kind: PREFLIGHT_AUTO_GC_SCOPE.to_string(),
            scope_id: PROJECT_SCOPE_ID.to_string(),
            holder: canonical.display().to_string(),
            holder_pid: None,
            heartbeat_secs: timestamp,
        },
    )
}

fn latest_auto_gc_at(root: &Path) -> Result<Option<u64>> {
    let canonical = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let conn = agent_doc_sqlite::state_store::open_state_db(&canonical)?;
    Ok(
        agent_doc_sqlite::state_store::load_coordination_lease_from_db(
            &conn,
            PREFLIGHT_AUTO_GC_SCOPE,
            PROJECT_SCOPE_ID,
        )?
        .map(|lease| lease.heartbeat_secs),
    )
}

fn current_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
