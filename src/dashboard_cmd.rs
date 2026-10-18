use anyhow::{Context, Result};
use std::path::Path;

use agent_doc_controller::fleet::{
    ActorListRecord, ActorListRegistryBinding, DashboardActorDiagnostics,
};
use agent_doc_controller_io::dashboard::DashboardEffects;

pub use agent_doc_controller_io::dashboard::DEFAULT_INTERVAL_MS;

struct CliDashboardEffects;

const EFFECTS: CliDashboardEffects = CliDashboardEffects;

impl DashboardEffects for CliDashboardEffects {
    fn actor_records(&self, root: &Path) -> Result<Vec<ActorListRecord>> {
        agent_doc_controller_io::project_controller::load_actor_list_records(root)
    }

    fn registry_bindings(&self, root: &Path) -> Result<Vec<ActorListRegistryBinding>> {
        let registry = agent_doc_session_registry_io::load_in(root)?;
        Ok(registry
            .values()
            .map(|entry| ActorListRegistryBinding {
                session_id: entry.session_id.clone(),
                cwd: entry.cwd.clone(),
            })
            .collect())
    }

    fn pane_alive(&self, pane: &str) -> bool {
        agent_doc_tmux_io::configured_tmux().pane_alive(pane)
    }

    fn actor_diagnostics(&self, root: &Path, document: &Path) -> Result<DashboardActorDiagnostics> {
        let inspection = agent_doc_controller_io::project_controller::inspect_actor(
            root,
            Some(document),
            None,
            None,
        )
        .with_context(|| {
            format!(
                "failed to inspect controller diagnostics for {}",
                document.display()
            )
        })?;
        Ok(DashboardActorDiagnostics {
            queue_control_state: inspection
                .queue_control
                .as_ref()
                .map(|control| control.state.clone()),
            queue_pressure: inspection
                .queue_backpressure
                .first()
                .map(|pressure| pressure.capacity_class.clone()),
            projection_lag: inspection.projection_lag,
        })
    }
}

pub fn dashboard(
    project_root: Option<&Path>,
    json: bool,
    once: bool,
    interval_ms: u64,
) -> Result<()> {
    agent_doc_controller_io::dashboard::dashboard(&EFFECTS, project_root, json, once, interval_ms)
}

/// `gvqv`: the controller liveness model without per-actor controller
/// inspection, for the `agent-doc dashboard` projection.
pub(crate) fn fleet_model_without_diagnostics(
    root: &Path,
) -> Result<agent_doc_controller::fleet::DashboardModel> {
    agent_doc_controller_io::dashboard::snapshot_model_without_diagnostics(&EFFECTS, root)
}
