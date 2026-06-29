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
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::admin::{AdminActor, AdminFinding};
use crate::sessions;
use tmux_router::Tmux;

/// Default refresh interval for the live dashboard loop (~1s, per the plan).
pub const DEFAULT_INTERVAL_MS: u64 = 1000;

/// One rendered dashboard row: an enumerated actor plus the finding kinds that
/// implicate it. `problem` is the highlight flag the view paints.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DashboardRow {
    #[serde(flatten)]
    pub actor: AdminActor,
    /// Distinct `admin detect` finding kinds naming this actor's document or pane.
    pub highlight_kinds: Vec<String>,
    /// Effective queue control state, when the controller has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_control_state: Option<String>,
    /// Latest typed queue pressure class, when dispatch was blocked/rejected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_pressure: Option<String>,
    /// Whether projection diagnostics report non-completed repair/emit state.
    #[serde(default)]
    pub projection_lag: bool,
    /// Whether this row is flagged (any finding implicates it).
    pub problem: bool,
}

/// Controller diagnostics attached to a dashboard actor row.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct DashboardActorDiagnostics {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_control_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_pressure: Option<String>,
    #[serde(default)]
    pub projection_lag: bool,
}

/// The full dashboard model: highlighted rows plus the raw findings that drove
/// the highlighting, so the view can render a summary footer.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DashboardModel {
    pub rows: Vec<DashboardRow>,
    pub findings: Vec<AdminFinding>,
    pub problem_count: usize,
}

/// Fold enumerated actors + derived findings into the deterministic view model.
///
/// Pure over its inputs (no I/O, no clock, no tmux), so the highlight logic is
/// unit-testable. A row is flagged when its `document_id` appears in any
/// finding's `documents`, or when the finding's `pane` matches the row's pane.
/// Rows are sorted by `document_id` for stable rendering across ticks.
pub fn build_dashboard_model(
    actors: Vec<AdminActor>,
    findings: Vec<AdminFinding>,
) -> DashboardModel {
    build_dashboard_model_with_diagnostics(actors, findings, BTreeMap::new())
}

/// Build a dashboard model with controller queue/projection diagnostics.
pub fn build_dashboard_model_with_diagnostics(
    mut actors: Vec<AdminActor>,
    findings: Vec<AdminFinding>,
    diagnostics: BTreeMap<String, DashboardActorDiagnostics>,
) -> DashboardModel {
    actors.sort_by(|a, b| {
        a.document_id
            .cmp(&b.document_id)
            .then_with(|| a.pane.cmp(&b.pane))
    });

    let rows: Vec<DashboardRow> = actors
        .into_iter()
        .map(|actor| {
            let mut highlight_kinds: Vec<String> = findings
                .iter()
                .filter(|f| {
                    f.documents.contains(&actor.document_id)
                        || f.pane.as_deref() == Some(actor.pane.as_str())
                })
                .map(|f| f.kind.clone())
                .collect();
            highlight_kinds.sort();
            highlight_kinds.dedup();
            let diagnostics = diagnostics
                .get(&actor.document_id)
                .cloned()
                .unwrap_or_default();
            let problem = !highlight_kinds.is_empty()
                || diagnostics.queue_pressure.is_some()
                || diagnostics.projection_lag;
            DashboardRow {
                actor,
                highlight_kinds,
                queue_control_state: diagnostics.queue_control_state,
                queue_pressure: diagnostics.queue_pressure,
                projection_lag: diagnostics.projection_lag,
                problem,
            }
        })
        .collect();

    let problem_count = rows.iter().filter(|r| r.problem).count();
    DashboardModel {
        rows,
        findings,
        problem_count,
    }
}

const RED: &str = "\x1b[31m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

/// Render the dashboard model into a stable terminal frame.
///
/// `color` gates ANSI styling so tests can assert on plain text. The layout is
/// deterministic given the model, which keeps redraws flicker-stable and makes
/// the render unit-testable.
pub fn render_dashboard(model: &DashboardModel, color: bool) -> String {
    let (red, bold, dim, reset) = if color {
        (RED, BOLD, DIM, RESET)
    } else {
        ("", "", "", "")
    };

    let mut out = String::new();
    out.push_str(&format!(
        "{bold}agent-doc fleet — {} actor(s), {} flagged{reset}\n",
        model.rows.len(),
        model.problem_count,
    ));

    if model.rows.is_empty() {
        out.push_str(&format!("{dim}  (no registered actors){reset}\n"));
        return out;
    }

    out.push_str(&format!(
        "{dim}  {:<2} {:<28} {:<9} {:<6} {:<6} {:<13} {:<8} {:<13} {:<5} {}{reset}\n",
        "", "document", "harness", "pane", "alive", "state", "pid", "queue", "proj", "flags",
    ));

    for row in &model.rows {
        let marker = if row.problem { "!" } else { " " };
        let pid = row
            .actor
            .supervisor_pid
            .map(|p| p.to_string())
            .unwrap_or_else(|| "?".to_string());
        let alive = if row.actor.pane_alive {
            "alive"
        } else {
            "dead"
        };
        let flags = row_flags(row).join(",");
        let queue = queue_summary(row);
        let projection = if row.projection_lag { "lag" } else { "ok" };
        let line = format!(
            "  {:<2} {:<28} {:<9} {:<6} {:<6} {:<13} {:<8} {:<13} {:<5} {}",
            marker,
            truncate(&row.actor.document_id, 28),
            truncate(&row.actor.harness, 9),
            truncate(&row.actor.pane, 6),
            alive,
            truncate(
                &format!("{} g{}", row.actor.state, row.actor.generation),
                13
            ),
            truncate(&pid, 8),
            truncate(&queue, 13),
            projection,
            flags,
        );
        if row.problem {
            out.push_str(&format!("{red}{line}{reset}\n"));
        } else {
            out.push_str(&line);
            out.push('\n');
        }
    }

    if !model.findings.is_empty() {
        out.push_str(&format!("\n{bold}findings:{reset}\n"));
        for f in &model.findings {
            out.push_str(&format!("{red}  [{}] {}{reset}\n", f.kind, f.detail));
        }
    }

    out
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    if max <= 1 {
        return s.chars().take(max).collect();
    }
    let head: String = s.chars().take(max - 1).collect();
    format!("{head}…")
}

fn queue_summary(row: &DashboardRow) -> String {
    if let Some(pressure) = row.queue_pressure.as_deref() {
        return format!("pressure:{pressure}");
    }
    row.queue_control_state
        .clone()
        .unwrap_or_else(|| "ok".to_string())
}

fn row_flags(row: &DashboardRow) -> Vec<String> {
    let mut flags = row.highlight_kinds.clone();
    if let Some(pressure) = row.queue_pressure.as_deref() {
        flags.push(format!("queue_pressure:{pressure}"));
    }
    if row.projection_lag {
        flags.push("projection_lag".to_string());
    }
    flags
}

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
    let rows = crate::admin::build_actor_list(&actors, &registry, |pane| tmux.pane_alive(pane));
    let findings = crate::admin::detect_findings(&actors, |pane| tmux.pane_alive(pane));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin::{AdminActor, AdminFinding};

    fn actor(document_id: &str, pane: &str, state: &str, alive: bool) -> AdminActor {
        AdminActor {
            document_id: document_id.to_string(),
            session_id: format!("sid-{document_id}"),
            pane: pane.to_string(),
            window: "@1".to_string(),
            harness: "codex".to_string(),
            generation: 1,
            state: state.to_string(),
            pane_alive: alive,
            supervisor_pid: Some(1234),
            cwd: Some("/proj".to_string()),
        }
    }

    #[test]
    fn model_sorts_rows_and_flags_nothing_on_clean_fleet() {
        let actors = vec![
            actor("tasks/b.md", "%2", "ready", true),
            actor("tasks/a.md", "%1", "busy", true),
        ];
        let model = build_dashboard_model(actors, vec![]);
        assert_eq!(model.rows.len(), 2);
        // Sorted by document_id.
        assert_eq!(model.rows[0].actor.document_id, "tasks/a.md");
        assert_eq!(model.rows[1].actor.document_id, "tasks/b.md");
        assert_eq!(model.problem_count, 0);
        assert!(model.rows.iter().all(|r| !r.problem));
    }

    #[test]
    fn cross_document_finding_flags_both_rows() {
        let actors = vec![
            actor("tasks/agent-doc-bugs2.md", "%70", "busy", true),
            actor("tasks/lazily-rs.md", "%70", "ready", true),
            actor("tasks/other.md", "%5", "ready", true),
        ];
        let findings = vec![AdminFinding {
            kind: "cross_document_pane".to_string(),
            detail: "pane %70 is the live binding of 2 documents".to_string(),
            documents: vec![
                "tasks/agent-doc-bugs2.md".to_string(),
                "tasks/lazily-rs.md".to_string(),
            ],
            pane: Some("%70".to_string()),
        }];
        let model = build_dashboard_model(actors, findings);
        assert_eq!(model.problem_count, 2);

        let flagged: Vec<&str> = model
            .rows
            .iter()
            .filter(|r| r.problem)
            .map(|r| r.actor.document_id.as_str())
            .collect();
        assert_eq!(
            flagged,
            vec!["tasks/agent-doc-bugs2.md", "tasks/lazily-rs.md"]
        );

        let clean = model
            .rows
            .iter()
            .find(|r| r.actor.document_id == "tasks/other.md")
            .unwrap();
        assert!(!clean.problem);
        assert!(clean.highlight_kinds.is_empty());

        let flagged_row = model
            .rows
            .iter()
            .find(|r| r.actor.document_id == "tasks/lazily-rs.md")
            .unwrap();
        assert_eq!(flagged_row.highlight_kinds, vec!["cross_document_pane"]);
    }

    #[test]
    fn stale_dead_pane_finding_flags_by_pane_match() {
        let actors = vec![actor("tasks/a.md", "%9", "busy", false)];
        let findings = vec![AdminFinding {
            kind: "stale_dead_pane".to_string(),
            detail: "orphaned binding".to_string(),
            documents: vec!["tasks/a.md".to_string()],
            pane: Some("%9".to_string()),
        }];
        let model = build_dashboard_model(actors, findings);
        assert_eq!(model.problem_count, 1);
        assert_eq!(model.rows[0].highlight_kinds, vec!["stale_dead_pane"]);
    }

    #[test]
    fn render_is_deterministic_plain_text() {
        let actors = vec![actor("tasks/a.md", "%1", "busy", true)];
        let model = build_dashboard_model(actors, vec![]);
        let frame = render_dashboard(&model, false);
        assert!(frame.contains("1 actor(s), 0 flagged"));
        assert!(frame.contains("tasks/a.md"));
        assert!(!frame.contains('\x1b'), "plain render must carry no ANSI");
    }

    #[test]
    fn render_flagged_row_carries_findings_footer_and_marker() {
        let actors = vec![actor("tasks/a.md", "%70", "busy", true)];
        let findings = vec![AdminFinding {
            kind: "cross_document_pane".to_string(),
            detail: "pane %70 is the live binding of 2 documents".to_string(),
            documents: vec!["tasks/a.md".to_string()],
            pane: Some("%70".to_string()),
        }];
        let model = build_dashboard_model(actors, findings);
        let frame = render_dashboard(&model, false);
        assert!(frame.contains("1 actor(s), 1 flagged"));
        assert!(frame.contains("cross_document_pane"));
        assert!(frame.contains("findings:"));
        // The flagged row carries the `!` marker.
        assert!(
            frame
                .lines()
                .any(|l| l.contains("tasks/a.md") && l.contains('!'))
        );
    }

    #[test]
    fn queue_pressure_and_projection_lag_flag_dashboard_row() {
        let actors = vec![actor("tasks/a.md", "%1", "ready", true)];
        let mut diagnostics = BTreeMap::new();
        diagnostics.insert(
            "tasks/a.md".to_string(),
            DashboardActorDiagnostics {
                queue_control_state: Some("paused".to_string()),
                queue_pressure: Some("queue_full".to_string()),
                projection_lag: true,
            },
        );

        let model = build_dashboard_model_with_diagnostics(actors, vec![], diagnostics);

        assert_eq!(model.problem_count, 1);
        assert_eq!(model.rows[0].queue_control_state.as_deref(), Some("paused"));
        assert_eq!(model.rows[0].queue_pressure.as_deref(), Some("queue_full"));
        assert!(model.rows[0].projection_lag);

        let frame = render_dashboard(&model, false);
        assert!(frame.contains("lag"));
        assert!(frame.contains("queue_pressure:queue_full"));
        assert!(frame.contains("projection_lag"));
    }

    #[test]
    fn empty_fleet_renders_placeholder() {
        let model = build_dashboard_model(vec![], vec![]);
        let frame = render_dashboard(&model, false);
        assert!(frame.contains("0 actor(s), 0 flagged"));
        assert!(frame.contains("no registered actors"));
    }

    #[test]
    fn truncate_keeps_unicode_safe() {
        assert_eq!(truncate("abc", 5), "abc");
        assert_eq!(truncate("abcdef", 4), "abc…");
        // Multi-byte chars must not panic on the boundary.
        let s = "αβγδεζ";
        let t = truncate(s, 3);
        assert_eq!(t.chars().count(), 3);
    }
}
