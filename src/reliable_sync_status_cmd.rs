//! `agent-doc reliable-sync-status` output (GH #62).
//!
//! Besides the reliable-sync plane, this surfaces the editor-surface auto-sync
//! state that was previously only inferable from coalesced ops.log lines: the
//! last surface observation the controller received, the last pane-layout
//! outcome, where each registered document's tmux pane lives, and whether the
//! visible editor documents diverge from that placement.

use std::path::Path;

use agent_doc_controller_io::project_controller::ControllerReliableSyncStatusResponse;
use agent_doc_editor_surface::pane_placement::{
    PanePlacement, PaneWindow, PlacementFinding, surface_pane_divergence,
};
use anyhow::Result;
use serde::Serialize;

#[derive(Serialize)]
struct StatusReport<'a> {
    #[serde(flatten)]
    status: &'a ControllerReliableSyncStatusResponse,
    pane_placements: &'a [PanePlacement],
    surface_pane_findings: &'a [PlacementFinding],
}

pub fn run(project_root: &Path, json: bool, check: bool) -> Result<()> {
    let status = agent_doc_controller_io::project_controller::reliable_sync_status(project_root)?;
    let registered: Vec<String> = status
        .registrations
        .iter()
        .map(|registration| normalize_document(project_root, &registration.path))
        .collect();
    let visible = project_visible_documents(project_root, &status);
    let mut tracked = registered.clone();
    for document in visible.iter().flatten() {
        if !tracked.contains(document) {
            tracked.push(document.clone());
        }
    }
    let placements = collect_pane_placements(project_root, &tracked);
    let findings = visible
        .as_ref()
        .map(|visible| surface_pane_divergence(visible, &placements))
        .unwrap_or_default();
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&StatusReport {
                status: &status,
                pane_placements: &placements,
                surface_pane_findings: &findings,
            })?
        );
        return check_outcome(check, &findings);
    }
    print_plane(&status);
    print_surface_sync(&status, now_ms());
    print_pane_placement(&registered, &placements);
    print_findings(&status, &findings);
    check_outcome(check, &findings)
}

/// GH #62: `--check` turns the divergence verdict into an exit status, so a doctor
/// script or CI step can fail on "the document the editor shows has its pane parked
/// in stash" instead of an operator reading three views by hand.
fn check_outcome(check: bool, findings: &[PlacementFinding]) -> Result<()> {
    if check && !findings.is_empty() {
        anyhow::bail!(
            "surface/pane divergence: {} finding(s); the tmux layout does not match the editor's visible documents",
            findings.len()
        );
    }
    Ok(())
}

/// Visible documents of the last ACCEPTED observation that belong to THIS
/// project. A rejected observation is not what the layout owner projects from,
/// and an editor surface spans every project it has open: a document under a
/// nested project root (its own controller, its own panes) is not this
/// project's to place. `None` when there is no accepted observation.
fn project_visible_documents(
    project_root: &Path,
    status: &ControllerReliableSyncStatusResponse,
) -> Option<Vec<String>> {
    let observation = status
        .surface_sync
        .as_ref()
        .and_then(|sync| sync.last_observation.as_ref())
        .filter(|observation| observation.accepted)?;
    let root = normalize_document(project_root, ".");
    Some(
        observation
            .visible
            .iter()
            .map(|document| normalize_document(project_root, document))
            .filter(|document| {
                agent_doc_fs::find_project_root(Path::new(document))
                    .map(|owner| normalize_document(project_root, &owner.to_string_lossy()))
                    .is_some_and(|owner| owner == root)
            })
            .collect(),
    )
}

fn normalize_document(project_root: &Path, document: &str) -> String {
    let path = Path::new(document);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        project_root.join(path)
    };
    std::fs::canonicalize(&absolute)
        .unwrap_or(absolute)
        .to_string_lossy()
        .into_owned()
}

/// Every tmux pane whose agent-doc owner process binds one of `documents`.
/// A tmux failure (no server) yields no placements rather than an error: the
/// status command must still report the plane.
fn collect_pane_placements(project_root: &Path, documents: &[String]) -> Vec<PanePlacement> {
    let runner = agent_doc_tmux_io::ProcessTmuxRunner::default_binary();
    let Ok(listing) = agent_doc_tmux_io::list_panes_all(
        &runner,
        "#{session_name}:#{window_index}\t#{window_name}\t#{pane_id}\t#{pane_pid}",
    ) else {
        return Vec::new();
    };
    listing
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let window_target = fields.next()?;
            let window_name = fields.next()?;
            let pane_id = fields.next()?;
            let pane_pid = fields.next()?;
            let owner =
                agent_doc_process_owner_io::process_tree_agent_doc_owner_document(pane_pid)?;
            let document = normalize_document(project_root, &owner);
            documents.contains(&document).then(|| PanePlacement {
                document,
                pane_id: pane_id.to_string(),
                window_target: window_target.to_string(),
                window: PaneWindow::from_window_name(window_name),
            })
        })
        .collect()
}

fn print_plane(status: &ControllerReliableSyncStatusResponse) {
    println!("authority: Lazily current + reliable-sync");
    println!("plane open docs ({}):", status.plane_open_docs.len());
    for doc in &status.plane_open_docs {
        let pids = status
            .per_doc_pids
            .iter()
            .find(|(d, _)| d == doc)
            .map(|(_, p)| p.clone())
            .unwrap_or_default();
        let live = if status.plane_live_docs.contains(doc) {
            "live"
        } else {
            "not-live"
        };
        let path = status
            .plane_open_paths
            .iter()
            .find(|(h, _)| h == doc)
            .and_then(|(_, p)| p.clone())
            .unwrap_or_else(|| "<registration pending>".to_string());
        println!("  {live:8}  pids={pids:?}  {path}");
    }
    println!(
        "live editor registrations ({}):",
        status.registrations.len()
    );
    for registration in &status.registrations {
        println!(
            "  {} pid={} {} {} {}",
            registration.editor_id,
            registration.pid,
            registration.editor_kind,
            registration.editor_version,
            registration.path
        );
    }
    println!(
        "in-memory registry open docs — secondary, empty right after a recycle ({}):",
        status.registry_open_docs.len()
    );
    for doc in &status.registry_open_docs {
        println!("  {doc}");
    }
}

fn print_surface_sync(status: &ControllerReliableSyncStatusResponse, now_ms: u64) {
    println!("editor surface auto-sync:");
    let Some(surface_sync) = &status.surface_sync else {
        println!(
            "  unavailable: the running controller predates this diagnostic; recycle it (`agent-doc admin recycle`)"
        );
        return;
    };
    match &surface_sync.last_observation {
        None => println!(
            "  last observation: none since this controller started — if the editor has changed tabs or splits since, it is not publishing its surface to this project"
        ),
        Some(observation) => {
            println!(
                "  last observation: {} ago  client={} gen={} seq={} accepted={} intent={} received={}",
                format_age(now_ms, observation.observed_at_ms),
                observation.client_id,
                observation.generation,
                observation.sequence,
                observation.accepted,
                observation.intent,
                observation.received_count,
            );
            println!("    focused: {}", display_or_none(&observation.focused));
            for document in &observation.visible {
                println!("    visible: {document}");
            }
        }
    }
    match &surface_sync.last_layout_outcome {
        None => println!("  last layout outcome: none since controller start"),
        Some(outcome) => {
            println!(
                "  last layout outcome: {} ago  gen={} attempt={} phase={}",
                format_age(now_ms, outcome.recorded_at_ms),
                outcome.generation,
                outcome.attempt,
                outcome.phase,
            );
            println!("    expected: {:?}", outcome.expected_documents);
            println!("    actual:   {:?}", outcome.actual_documents);
        }
    }
}

fn print_pane_placement(registered: &[String], placements: &[PanePlacement]) {
    println!("tmux pane placement for registered documents:");
    for document in registered {
        let panes: Vec<&PanePlacement> = placements
            .iter()
            .filter(|placement| &placement.document == document)
            .collect();
        if panes.is_empty() {
            println!("  <no pane>  {document}");
        }
        for pane in panes {
            println!(
                "  {:10} {} {}  {document}",
                pane.window.label(),
                pane.window_target,
                pane.pane_id
            );
        }
    }
}

fn print_findings(status: &ControllerReliableSyncStatusResponse, findings: &[PlacementFinding]) {
    let accepted = status
        .surface_sync
        .as_ref()
        .and_then(|sync| sync.last_observation.as_ref())
        .is_some_and(|observation| observation.accepted);
    if !accepted {
        println!("surface/pane divergence: not evaluated (no accepted surface observation)");
        return;
    }
    if findings.is_empty() {
        println!("surface/pane divergence: none");
        return;
    }
    println!("surface/pane divergence ({}):", findings.len());
    for finding in findings {
        println!("  DIVERGED: {}", finding.describe());
    }
}

fn display_or_none(value: &str) -> &str {
    if value.is_empty() { "<none>" } else { value }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

fn format_age(now_ms: u64, then_ms: u64) -> String {
    let seconds = now_ms.saturating_sub(then_ms) / 1000;
    match seconds {
        0..=119 => format!("{seconds}s"),
        120..=7199 => format!("{}m", seconds / 60),
        _ => format!("{}h", seconds / 3600),
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn check_fails_only_on_a_divergence_and_only_when_asked() {
        // GH #62
        let finding = PlacementFinding::VisibleDocumentStashed {
            document: "/p/tasks/agent-doc.md".to_string(),
            pane_id: "%28".to_string(),
        };
        assert!(check_outcome(true, std::slice::from_ref(&finding)).is_err());
        assert!(check_outcome(false, &[finding]).is_ok());
        assert!(check_outcome(true, &[]).is_ok());
    }

    use super::*;

    #[test]
    fn format_age_scales_units() {
        assert_eq!(format_age(10_000, 4_000), "6s");
        assert_eq!(format_age(600_000, 0), "10m");
        assert_eq!(format_age(10_800_000, 0), "3h");
        assert_eq!(format_age(0, 5_000), "0s");
    }

    fn status_with_visible(
        accepted: bool,
        visible: Vec<String>,
    ) -> ControllerReliableSyncStatusResponse {
        use agent_doc_controller_io::project_controller::{
            ControllerSurfaceSyncDiagnostics, SurfaceObservationDiagnostic,
        };
        ControllerReliableSyncStatusResponse {
            plane_open_docs: Vec::new(),
            plane_open_paths: Vec::new(),
            plane_live_docs: Vec::new(),
            registrations: Vec::new(),
            allocated_model_docs: Vec::new(),
            allocated_model_projection_complete: true,
            registry_open_docs: Vec::new(),
            per_doc_pids: Vec::new(),
            surface_sync: Some(ControllerSurfaceSyncDiagnostics {
                last_observation: Some(SurfaceObservationDiagnostic {
                    client_id: "jetbrains-pid:1".to_string(),
                    generation: 1,
                    sequence: 1,
                    accepted,
                    intent: "sync".to_string(),
                    focused: String::new(),
                    visible,
                    observed_at_ms: 0,
                    received_count: 1,
                }),
                last_layout_outcome: None,
            }),
        }
    }

    /// The editor surface spans every project it has open; a document under a
    /// nested project root belongs to that project's controller and panes.
    #[test]
    fn visible_documents_exclude_nested_project_roots() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("outer");
        let nested = root.join("src/inner");
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(nested.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::create_dir_all(nested.join("tasks")).unwrap();
        let own = root.join("tasks/a.md");
        let foreign = nested.join("tasks/b.md");
        std::fs::write(&own, "").unwrap();
        std::fs::write(&foreign, "").unwrap();
        let status = status_with_visible(
            true,
            vec![
                own.to_string_lossy().into_owned(),
                foreign.to_string_lossy().into_owned(),
            ],
        );
        let visible = project_visible_documents(&root, &status).unwrap();
        assert_eq!(visible, vec![normalize_document(&root, "tasks/a.md")]);
    }

    #[test]
    fn rejected_observation_is_not_evaluated() {
        let status = status_with_visible(false, vec!["/p/a.md".to_string()]);
        assert!(project_visible_documents(Path::new("/p"), &status).is_none());
    }

    #[test]
    fn controller_predating_diagnostic_is_not_evaluated() {
        let mut status = status_with_visible(true, Vec::new());
        status.surface_sync = None;
        assert!(project_visible_documents(Path::new("/p"), &status).is_none());
    }

    #[test]
    fn normalize_document_joins_relative_paths_to_root() {
        let root = Path::new("/nonexistent-agent-doc-root");
        assert_eq!(
            normalize_document(root, "tasks/a.md"),
            "/nonexistent-agent-doc-root/tasks/a.md"
        );
        assert_eq!(normalize_document(root, "/abs/b.md"), "/abs/b.md");
    }
}
