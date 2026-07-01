//! Live operator dashboard over the `#ipc-admin-api` read surfaces
//! (`#actor-runtime-dashboard` / queue id `#admin-fleet-dashboard`).
//!
//! This is a view over `admin list` / `admin detect` plus controller-backed
//! `inspect` diagnostics; it adds no new control logic. `build_dashboard_model`
//! folds the enumerated actor rows together with derived findings and optional
//! queue/projection diagnostics into a deterministic model (one row per actor),
//! and `render_dashboard` turns that model into a stable terminal frame. The
//! model/view split keeps the highlight logic unit-testable without a live
//! terminal (the split the plan calls for) while the `dashboard` command owns
//! the polling loop and screen redraw.
//!
//! Mutating row keybinds (`admin stop` / `admin rebind`) depend on the gated
//! `#ipc-admin-api` Phase 2 verbs and are intentionally not implemented here;
//! Phase 1 is read-only. See `tasks/agent-doc/plan-actor-runtime-dashboard.md`.

use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::sessions;
use agent_doc_controller::fleet::{
    ActorListRecord, ActorListRegistryBinding, AdminActor, DashboardActorDiagnostics,
    DashboardModel, build_admin_actor_list, build_dashboard_model_with_diagnostics,
    detect_admin_findings, render_dashboard,
};
use tmux_router::Tmux;

/// Default refresh interval for the live dashboard loop (~1s, per the plan).
pub const DEFAULT_INTERVAL_MS: u64 = 1000;

const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

/// Resolve the project root, mirroring `admin`'s resolution.
fn resolve_root(project_root: Option<&Path>) -> Result<PathBuf> {
    if let Some(root) = project_root {
        return Ok(root.to_path_buf());
    }
    let cwd = std::env::current_dir().context("failed to read current directory")?;
    agent_doc_fs::find_project_root(&cwd)
        .with_context(|| format!("no .agent-doc project root found from {}", cwd.display()))
}

/// Read a fresh dashboard model from disk + live tmux liveness.
fn snapshot_model(root: &Path) -> Result<DashboardModel> {
    let actors = crate::project_controller::load_actor_store(root)?;
    let registry = sessions::load_in(root)?;
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
    let diagnostics = snapshot_controller_diagnostics(root, &rows)?;
    Ok(build_dashboard_model_with_diagnostics(
        rows,
        findings,
        diagnostics,
    ))
}

fn snapshot_controller_diagnostics(
    root: &Path,
    rows: &[AdminActor],
) -> Result<BTreeMap<String, DashboardActorDiagnostics>> {
    let mut diagnostics = BTreeMap::new();
    for row in rows {
        let document = Path::new(&row.document_id);
        let inspection = crate::project_controller::inspect_actor(root, Some(document), None, None)
            .with_context(|| {
                format!(
                    "failed to inspect controller diagnostics for {}",
                    row.document_id
                )
            })?;
        diagnostics.insert(
            row.document_id.clone(),
            DashboardActorDiagnostics {
                queue_control_state: inspection
                    .queue_control
                    .as_ref()
                    .map(|control| control.state.clone()),
                queue_pressure: inspection
                    .queue_backpressure
                    .first()
                    .map(|pressure| pressure.capacity_class.clone()),
                projection_lag: inspection.projection_lag,
            },
        );
    }
    Ok(diagnostics)
}

/// `agent-doc admin dashboard` — live read-only fleet view.
///
/// - `--json` emits a single model snapshot and exits (scripting / piping).
/// - `--once` renders one terminal frame and exits (no polling loop).
/// - otherwise polls every `interval_ms`, clearing + redrawing the screen.
pub fn dashboard(
    project_root: Option<&Path>,
    json: bool,
    once: bool,
    interval_ms: u64,
) -> Result<()> {
    let root = resolve_root(project_root)?;

    if json {
        let model = snapshot_model(&root)?;
        println!("{}", serde_json::to_string_pretty(&model)?);
        return Ok(());
    }

    if once {
        let model = snapshot_model(&root)?;
        print!(
            "{}",
            render_dashboard(&model, std::io::IsTerminal::is_terminal(&std::io::stdout()))
        );
        return Ok(());
    }

    let color = std::io::IsTerminal::is_terminal(&std::io::stdout());
    let interval = Duration::from_millis(interval_ms.max(100));
    loop {
        let model = snapshot_model(&root)?;
        // Clear screen + home cursor, then draw the frame.
        print!("\x1b[2J\x1b[H");
        print!("{}", render_dashboard(&model, color));
        print!(
            "\n{}refresh {}ms · Ctrl-C to exit{}\n",
            if color { DIM } else { "" },
            interval_ms,
            if color { RESET } else { "" },
        );
        use std::io::Write;
        std::io::stdout().flush().ok();
        std::thread::sleep(interval);
    }
}
