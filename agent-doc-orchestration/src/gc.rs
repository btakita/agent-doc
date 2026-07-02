use anyhow::Result;
use std::path::Path;
use std::time::Duration;

pub use agent_doc_gc_io::GcResult;

struct OrchestrationGcEffects;

impl agent_doc_gc_io::GcControllerEffects for OrchestrationGcEffects {
    fn close_stale_starting_actors(
        &mut self,
        project_root: &Path,
        stale_after: Duration,
        dry_run: bool,
    ) -> Result<(usize, usize)> {
        crate::project_controller::close_stale_starting_actors(project_root, stale_after, dry_run)
    }

    fn close_stale_dead_pane_actors(
        &mut self,
        project_root: &Path,
        dry_run: bool,
        caller: &str,
        reason: &str,
    ) -> Result<(usize, usize)> {
        crate::project_controller::close_stale_dead_pane_actors_with_tmux_for_caller(
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
        crate::project_controller::prune_dead_actors(project_root, prune_after, dry_run)
    }

    fn terminate_stale_preparing_controllers(
        &mut self,
        project_root: &Path,
        stale_after: Duration,
        dry_run: bool,
    ) -> Result<(usize, usize)> {
        crate::project_controller::terminate_stale_preparing_controllers(
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
        crate::project_controller::reap_orphaned_preparing_controllers(
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
        crate::project_controller::reap_orphaned_preparing_controllers_all_projects(
            stale_after,
            dry_run,
            caller,
        )
    }
}

pub fn run(root: Option<&Path>, dry_run: bool) -> Result<GcResult> {
    let mut effects = OrchestrationGcEffects;
    agent_doc_gc_io::run_with_controller_effects(
        root,
        dry_run,
        &mut effects,
        agent_doc_gc_io::GcControllerConfig {
            stale_starting_after: Duration::from_secs(3600),
            dead_actor_prune_after: crate::project_controller::DEAD_ACTOR_PRUNE_AFTER,
            stale_preparing_controller_after:
                crate::project_controller::stale_preparing_controller_threshold(),
        },
    )
}
