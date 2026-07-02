//! Live controller fleet dashboard IO.
//!
//! The pure fleet model and terminal rendering live in `agent-doc-controller`.
//! This module owns project-root resolution, JSON/terminal output, polling, and
//! the snapshot assembly shape while callers inject their concrete actor store,
//! session registry, tmux liveness, and controller-inspect effects.

use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use agent_doc_controller::fleet::{
    ActorListRecord, ActorListRegistryBinding, AdminActor, DashboardActorDiagnostics,
    DashboardModel, build_admin_actor_list, build_dashboard_model_with_diagnostics,
    detect_admin_findings, render_dashboard,
};

/// Default refresh interval for the live dashboard loop (~1s, per the plan).
pub const DEFAULT_INTERVAL_MS: u64 = 1000;

const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

/// Effect boundary for producing a fresh controller fleet dashboard snapshot.
pub trait DashboardEffects {
    fn actor_records(&self, root: &Path) -> Result<Vec<ActorListRecord>>;
    fn registry_bindings(&self, root: &Path) -> Result<Vec<ActorListRegistryBinding>>;
    fn pane_alive(&self, pane: &str) -> bool;
    fn actor_diagnostics(&self, root: &Path, document: &Path) -> Result<DashboardActorDiagnostics>;
}

/// Resolve the project root, mirroring admin command resolution.
pub fn resolve_root(project_root: Option<&Path>) -> Result<PathBuf> {
    if let Some(root) = project_root {
        return Ok(root.to_path_buf());
    }
    let cwd = std::env::current_dir().context("failed to read current directory")?;
    agent_doc_fs::find_project_root(&cwd)
        .with_context(|| format!("no .agent-doc project root found from {}", cwd.display()))
}

/// Read a fresh dashboard model from the injected controller/session/tmux effects.
pub fn snapshot_model<E: DashboardEffects + ?Sized>(
    effects: &E,
    root: &Path,
) -> Result<DashboardModel> {
    let rows = build_admin_actor_list(
        effects.actor_records(root)?,
        effects.registry_bindings(root)?,
        |pane| effects.pane_alive(pane),
    );
    let findings = detect_admin_findings(&rows);
    let diagnostics = snapshot_controller_diagnostics(effects, root, &rows)?;
    Ok(build_dashboard_model_with_diagnostics(
        rows,
        findings,
        diagnostics,
    ))
}

fn snapshot_controller_diagnostics<E: DashboardEffects + ?Sized>(
    effects: &E,
    root: &Path,
    rows: &[AdminActor],
) -> Result<BTreeMap<String, DashboardActorDiagnostics>> {
    let mut diagnostics = BTreeMap::new();
    for row in rows {
        let document = Path::new(&row.document_id);
        let diagnostic = effects.actor_diagnostics(root, document).with_context(|| {
            format!(
                "failed to inspect controller diagnostics for {}",
                row.document_id
            )
        })?;
        diagnostics.insert(row.document_id.clone(), diagnostic);
    }
    Ok(diagnostics)
}

/// `agent-doc admin dashboard` - live read-only fleet view.
///
/// - `--json` emits a single model snapshot and exits (scripting / piping).
/// - `--once` renders one terminal frame and exits (no polling loop).
/// - otherwise polls every `interval_ms`, clearing + redrawing the screen.
pub fn dashboard<E: DashboardEffects + ?Sized>(
    effects: &E,
    project_root: Option<&Path>,
    json: bool,
    once: bool,
    interval_ms: u64,
) -> Result<()> {
    let root = resolve_root(project_root)?;

    if json {
        let model = snapshot_model(effects, &root)?;
        println!("{}", serde_json::to_string_pretty(&model)?);
        return Ok(());
    }

    if once {
        let model = snapshot_model(effects, &root)?;
        print!(
            "{}",
            render_dashboard(&model, std::io::stdout().is_terminal())
        );
        return Ok(());
    }

    let color = std::io::stdout().is_terminal();
    let interval = Duration::from_millis(interval_ms.max(100));
    loop {
        let model = snapshot_model(effects, &root)?;
        print!("\x1b[2J\x1b[H");
        print!("{}", render_dashboard(&model, color));
        print!(
            "\n{}refresh {}ms - Ctrl-C to exit{}\n",
            if color { DIM } else { "" },
            interval_ms,
            if color { RESET } else { "" },
        );
        std::io::stdout().flush().ok();
        std::thread::sleep(interval);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    #[derive(Default)]
    struct FakeEffects {
        alive_panes: BTreeSet<String>,
        diagnostics: BTreeMap<String, DashboardActorDiagnostics>,
    }

    impl DashboardEffects for FakeEffects {
        fn actor_records(&self, _root: &Path) -> Result<Vec<ActorListRecord>> {
            Ok(vec![
                ActorListRecord {
                    document_id: "tasks/a.md".to_string(),
                    session_id: "session-a".to_string(),
                    pane: "%1".to_string(),
                    window: "@1".to_string(),
                    harness: "codex".to_string(),
                    generation: 1,
                    state: "running".to_string(),
                },
                ActorListRecord {
                    document_id: "tasks/b.md".to_string(),
                    session_id: "session-b".to_string(),
                    pane: "%2".to_string(),
                    window: "@1".to_string(),
                    harness: "claude".to_string(),
                    generation: 2,
                    state: "running".to_string(),
                },
            ])
        }

        fn registry_bindings(&self, _root: &Path) -> Result<Vec<ActorListRegistryBinding>> {
            Ok(vec![ActorListRegistryBinding {
                session_id: "session-a".to_string(),
                supervisor_pid: 42,
                cwd: "/repo".to_string(),
            }])
        }

        fn pane_alive(&self, pane: &str) -> bool {
            self.alive_panes.contains(pane)
        }

        fn actor_diagnostics(
            &self,
            _root: &Path,
            document: &Path,
        ) -> Result<DashboardActorDiagnostics> {
            Ok(self
                .diagnostics
                .get(document.to_string_lossy().as_ref())
                .cloned()
                .unwrap_or_default())
        }
    }

    #[test]
    fn snapshot_model_builds_rows_findings_and_controller_diagnostics() {
        let mut effects = FakeEffects::default();
        effects.alive_panes.insert("%1".to_string());
        effects.diagnostics.insert(
            "tasks/b.md".to_string(),
            DashboardActorDiagnostics {
                queue_control_state: Some("paused".to_string()),
                queue_pressure: Some("full".to_string()),
                projection_lag: true,
            },
        );

        let model = snapshot_model(&effects, Path::new("/repo")).unwrap();

        assert_eq!(model.rows.len(), 2);
        assert_eq!(model.problem_count, 1);
        assert_eq!(model.rows[0].actor.document_id, "tasks/a.md");
        assert_eq!(model.rows[0].actor.supervisor_pid, Some(42));
        assert!(!model.rows[0].problem);
        assert_eq!(model.rows[1].actor.document_id, "tasks/b.md");
        assert!(
            model.rows[1]
                .highlight_kinds
                .contains(&"stale_dead_pane".to_string())
        );
        assert_eq!(model.rows[1].queue_control_state.as_deref(), Some("paused"));
        assert_eq!(model.rows[1].queue_pressure.as_deref(), Some("full"));
        assert!(model.rows[1].projection_lag);
        assert!(model.rows[1].problem);
    }
}
