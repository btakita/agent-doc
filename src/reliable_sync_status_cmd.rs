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
    unobserved_surface_gap: Option<&'a UnobservedSurfaceGap>,
}

/// GH #72: the first JetBrains plugin build whose `CpRouteClient` publishes the
/// editor surface (`observeEditorSurface`, 8ecb433e3).
const JETBRAINS_SURFACE_PUBLISHER_SINCE: (u64, u64, u64) = (0, 2, 334);

/// GH #72: what can still be said when the controller holds no accepted
/// surface observation. Without one the layout effect synthesises its
/// expectation from focus alone and the divergence check has no input, so both
/// read as success while registered documents sit in stash. The placement of
/// the registered documents is decidable without the editor, so it is reported
/// instead of staying silent.
#[derive(Debug, PartialEq, Eq, Serialize)]
struct UnobservedSurfaceGap {
    registered: usize,
    in_agent_doc: usize,
    in_stash: usize,
    elsewhere: usize,
    without_pane: usize,
    /// The last layout outcome's expectation was built without an accepted
    /// observation, i.e. from focus only.
    expectation_synthesised: bool,
    /// Registered documents absent from that synthesised expectation.
    registered_outside_expectation: usize,
    publisher: PublisherVerdict,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
enum PublisherVerdict {
    /// No live JetBrains registration to judge.
    Unknown,
    /// Every live JetBrains plugin ships the publisher, so a silent surface is
    /// a publishing/transport fault, not a missing feature.
    Ships { versions: Vec<String> },
    /// At least one live JetBrains plugin predates the publisher.
    Predates { versions: Vec<String> },
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
    let gap = unobserved_surface_gap(project_root, &status, &registered, &placements);
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&StatusReport {
                status: &status,
                pane_placements: &placements,
                surface_pane_findings: &findings,
                unobserved_surface_gap: gap.as_ref(),
            })?
        );
        return check_outcome(check, &findings);
    }
    print_plane(&status);
    print_surface_sync(&status, gap.as_ref(), now_ms());
    print_pane_placement(&registered, &placements);
    print_findings(&findings, gap.as_ref());
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

fn has_accepted_observation(status: &ControllerReliableSyncStatusResponse) -> bool {
    status
        .surface_sync
        .as_ref()
        .and_then(|sync| sync.last_observation.as_ref())
        .is_some_and(|observation| observation.accepted)
}

/// `None` when an accepted observation exists (the real divergence check
/// runs) or the controller predates the surface diagnostic entirely.
fn unobserved_surface_gap(
    project_root: &Path,
    status: &ControllerReliableSyncStatusResponse,
    registered: &[String],
    placements: &[PanePlacement],
) -> Option<UnobservedSurfaceGap> {
    let surface_sync = status.surface_sync.as_ref()?;
    if has_accepted_observation(status) {
        return None;
    }
    let mut gap = UnobservedSurfaceGap {
        registered: registered.len(),
        in_agent_doc: 0,
        in_stash: 0,
        elsewhere: 0,
        without_pane: 0,
        expectation_synthesised: false,
        registered_outside_expectation: 0,
        publisher: publisher_verdict(&status.registrations),
    };
    for document in registered {
        let windows: Vec<&PaneWindow> = placements
            .iter()
            .filter(|placement| &placement.document == document)
            .map(|placement| &placement.window)
            .collect();
        if windows.is_empty() {
            gap.without_pane += 1;
        } else if windows.contains(&&PaneWindow::AgentDoc) {
            gap.in_agent_doc += 1;
        } else if windows.contains(&&PaneWindow::Stash) {
            gap.in_stash += 1;
        } else {
            gap.elsewhere += 1;
        }
    }
    if let Some(outcome) = &surface_sync.last_layout_outcome {
        gap.expectation_synthesised = true;
        let expected: Vec<String> = outcome
            .expected_documents
            .iter()
            .map(|document| normalize_document(project_root, document))
            .collect();
        gap.registered_outside_expectation = registered
            .iter()
            .filter(|document| !expected.contains(document))
            .count();
    }
    Some(gap)
}

fn publisher_verdict(
    registrations: &[agent_doc_reliable_sync_io::liveness::EditorRegistration],
) -> PublisherVerdict {
    let mut versions: Vec<String> = registrations
        .iter()
        .filter(|registration| registration.editor_kind == "jetbrains")
        .map(|registration| registration.editor_version.clone())
        .collect();
    versions.sort();
    versions.dedup();
    if versions.is_empty() {
        return PublisherVerdict::Unknown;
    }
    let predates = versions.iter().any(|version| {
        parse_version(version).is_some_and(|parsed| parsed < JETBRAINS_SURFACE_PUBLISHER_SINCE)
    });
    if predates {
        PublisherVerdict::Predates { versions }
    } else {
        PublisherVerdict::Ships { versions }
    }
}

fn parse_version(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.trim().split(['.', '-', '+']);
    Some((
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    ))
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

fn print_surface_sync(
    status: &ControllerReliableSyncStatusResponse,
    gap: Option<&UnobservedSurfaceGap>,
    now_ms: u64,
) {
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
            if let Some(gap) = gap.filter(|gap| gap.expectation_synthesised) {
                println!(
                    "    expectation synthesised from focus — no accepted surface observation; {} registered document(s) not in expectation",
                    gap.registered_outside_expectation
                );
            }
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

fn print_findings(findings: &[PlacementFinding], gap: Option<&UnobservedSurfaceGap>) {
    if let Some(gap) = gap {
        for line in describe_gap(gap) {
            println!("{line}");
        }
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

fn describe_gap(gap: &UnobservedSurfaceGap) -> Vec<String> {
    let mut lines = vec![
        "surface/pane divergence: not evaluated against the editor (no accepted surface observation)"
            .to_string(),
        format!(
            "  placement without an observation: {} registered — {} in agent-doc, {} in stash, {} elsewhere, {} without a pane",
            gap.registered, gap.in_agent_doc, gap.in_stash, gap.elsewhere, gap.without_pane
        ),
    ];
    if gap.in_stash > 0 {
        lines.push(format!(
            "  UNVERIFIED: {} registered document(s) parked in stash; if the editor shows more than {} document(s), the layout is wrong and no check can see it",
            gap.in_stash, gap.in_agent_doc
        ));
    }
    match &gap.publisher {
        PublisherVerdict::Unknown => {}
        PublisherVerdict::Ships { versions } => lines.push(format!(
            "  publisher: live JetBrains plugin {versions:?} ships the surface publisher (since {}.{}.{}); it is silent, so suspect the editor→controller path, not the plugin version",
            JETBRAINS_SURFACE_PUBLISHER_SINCE.0,
            JETBRAINS_SURFACE_PUBLISHER_SINCE.1,
            JETBRAINS_SURFACE_PUBLISHER_SINCE.2,
        )),
        PublisherVerdict::Predates { versions } => lines.push(format!(
            "  publisher: live JetBrains plugin {versions:?} predates the surface publisher ({}.{}.{}); restart the editor to load a current plugin",
            JETBRAINS_SURFACE_PUBLISHER_SINCE.0,
            JETBRAINS_SURFACE_PUBLISHER_SINCE.1,
            JETBRAINS_SURFACE_PUBLISHER_SINCE.2,
        )),
    }
    lines
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

    fn registration(
        path: &str,
        version: &str,
    ) -> agent_doc_reliable_sync_io::liveness::EditorRegistration {
        agent_doc_reliable_sync_io::liveness::EditorRegistration {
            document_hash: path.to_string(),
            pid: 1506046,
            path: path.to_string(),
            editor_id: "jetbrains-pid:1506046".to_string(),
            editor_kind: "jetbrains".to_string(),
            editor_version: version.to_string(),
            capabilities: Vec::new(),
            timestamp_ms: 0,
        }
    }

    fn placement(document: &str, window: PaneWindow) -> PanePlacement {
        PanePlacement {
            document: document.to_string(),
            pane_id: format!("%{}", document.len()),
            window_target: "0:0".to_string(),
            window,
        }
    }

    /// GH #72: the operator's measured shape — seven registered documents, one
    /// in `agent-doc`, six in stash, no surface observation since controller
    /// start, and a `converged_focus_only` outcome whose expectation is the
    /// focused document alone.
    fn issue_72_status(version: &str) -> (ControllerReliableSyncStatusResponse, Vec<String>) {
        use agent_doc_controller_io::project_controller::{
            ControllerSurfaceSyncDiagnostics, PaneLayoutOutcomeDiagnostic,
        };
        let registered: Vec<String> = (0..7).map(|i| format!("/p/tasks/{i}.md")).collect();
        let mut status = status_with_visible(true, Vec::new());
        status.registrations = registered
            .iter()
            .map(|path| registration(path, version))
            .collect();
        status.surface_sync = Some(ControllerSurfaceSyncDiagnostics {
            last_observation: None,
            last_layout_outcome: Some(PaneLayoutOutcomeDiagnostic {
                generation: 2,
                attempt: 1,
                phase: "converged_focus_only".to_string(),
                expected_documents: vec![registered[0].clone()],
                actual_documents: vec![registered[0].clone()],
                recorded_at_ms: 0,
            }),
        });
        (status, registered)
    }

    fn issue_72_placements(registered: &[String]) -> Vec<PanePlacement> {
        registered
            .iter()
            .enumerate()
            .map(|(i, document)| {
                let window = if i == 0 {
                    PaneWindow::AgentDoc
                } else {
                    PaneWindow::Stash
                };
                placement(document, window)
            })
            .collect()
    }

    #[test]
    fn missing_observation_still_reports_stash_placement() {
        let (status, registered) = issue_72_status("0.2.392");
        let placements = issue_72_placements(&registered);
        let gap = unobserved_surface_gap(Path::new("/p"), &status, &registered, &placements)
            .expect("no accepted observation must yield a gap report, not silence");
        assert_eq!(gap.registered, 7);
        assert_eq!(gap.in_agent_doc, 1);
        assert_eq!(gap.in_stash, 6);
        assert_eq!(gap.without_pane, 0);
        assert!(gap.expectation_synthesised);
        assert_eq!(gap.registered_outside_expectation, 6);
        let lines = describe_gap(&gap).join("\n");
        assert!(
            lines.contains("UNVERIFIED: 6 registered document(s) parked in stash"),
            "{lines}"
        );
        assert!(lines.contains("ships the surface publisher"), "{lines}");
    }

    #[test]
    fn accepted_observation_suppresses_the_gap_report() {
        let (mut status, registered) = issue_72_status("0.2.392");
        status.surface_sync.as_mut().unwrap().last_observation =
            status_with_visible(true, Vec::new())
                .surface_sync
                .unwrap()
                .last_observation;
        let placements = issue_72_placements(&registered);
        assert!(
            unobserved_surface_gap(Path::new("/p"), &status, &registered, &placements).is_none()
        );
    }

    #[test]
    fn rejected_observation_still_reports_the_gap() {
        let (mut status, registered) = issue_72_status("0.2.392");
        status.surface_sync.as_mut().unwrap().last_observation =
            status_with_visible(false, Vec::new())
                .surface_sync
                .unwrap()
                .last_observation;
        let placements = issue_72_placements(&registered);
        assert!(
            unobserved_surface_gap(Path::new("/p"), &status, &registered, &placements).is_some()
        );
    }

    #[test]
    fn plugin_predating_the_publisher_is_named() {
        let (status, _) = issue_72_status("0.2.300");
        assert_eq!(
            publisher_verdict(&status.registrations),
            PublisherVerdict::Predates {
                versions: vec!["0.2.300".to_string()]
            }
        );
        assert_eq!(
            publisher_verdict(&[registration("/p/a.md", "0.2.334")]),
            PublisherVerdict::Ships {
                versions: vec!["0.2.334".to_string()]
            }
        );
        assert_eq!(publisher_verdict(&[]), PublisherVerdict::Unknown);
    }

    #[test]
    fn no_stash_placement_is_not_flagged_unverified() {
        let (status, registered) = issue_72_status("0.2.392");
        let placements: Vec<PanePlacement> = registered
            .iter()
            .map(|document| placement(document, PaneWindow::AgentDoc))
            .collect();
        let gap =
            unobserved_surface_gap(Path::new("/p"), &status, &registered, &placements).unwrap();
        assert!(!describe_gap(&gap).join("\n").contains("UNVERIFIED"));
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
