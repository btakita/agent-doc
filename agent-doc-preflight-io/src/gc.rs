//! Preflight automatic GC and stale actor cleanup adapters.

use anyhow::Result;
use std::path::Path;
use std::time::Duration;

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
/// stamp-gated to once per day under `.agent-doc/gc.stamp`.
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
    let stamp = root.join(".agent-doc/gc.stamp");
    let needs_gc = match std::fs::metadata(&stamp) {
        Ok(meta) => meta
            .modified()
            .ok()
            .and_then(|t| t.elapsed().ok())
            .map(|age| age > Duration::from_secs(86400))
            .unwrap_or(true),
        Err(_) => true,
    };
    if needs_gc {
        eprintln!("[preflight] step 0a: auto-gc");
        match run_auto_gc(&root) {
            Ok(result) => {
                if result.deleted > 0 {
                    eprintln!("[preflight] gc: {} files cleaned", result.deleted);
                }
                let _ = std::fs::write(&stamp, "");
            }
            Err(e) => eprintln!("[preflight] gc warning: {}", e),
        }
    }
}
