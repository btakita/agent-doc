//! Editor surface projection behind the hot-reloaded native boundary
//! (GH #326 phase 2, the #295 "path 2" lever).
//!
//! The JetBrains automatic surface (`EditorTabSyncListener.captureSurface`)
//! used to decide on the EDT, in Kotlin, which session documents are visible,
//! whether the editor projection had settled, which document is active, how
//! open tabs rank by proximity, how a stale selection edge settles, and what
//! payload reaches the Project Controller. A change to any of those rules
//! needed a plugin generation, and on the exact-263 modular package that is an
//! IDE restart. They live here now: the plugin captures what IntelliJ state
//! *is* ([`SurfaceProjectionEvidence`]) and the delivery worker asks the
//! `surface_projection` derivation what it *means*.
//!
//! [`project_surface`] is pure except for the Remote Dev fold, which keeps a
//! per-project memory and is injected by the IO crate. The fold runs only for a
//! projection that is ready to publish, exactly as before: a deferred capture
//! never advances the fold's memory.
//!
//! The plugin keeps a frozen Kotlin copy of these rules for a native library
//! that cannot answer; one parity fixture
//! (`editors/jetbrains/src/test/resources/editor-derivations/surface_projection.json`)
//! is answered by both. New rules land here, never there.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::window_layout::{
    EditorWindowEvidence, WindowLayoutEvidence, WindowSnapshot, observed_layout_line,
    resolve_window_layout, session_tabs_mru_last, sticky_session_document,
};
use crate::{
    EditorSurface, RemoteClientEditors, RemoteLayoutEvidence, RemoteLayoutResolution,
    SurfaceColumn, SurfaceColumnOrder,
};

/// Remote Dev client state captured on the EDT (GH #134 / #157). Paths are
/// project relative, as the remote fold publishes them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteSurfaceEvidence {
    /// The key the remote fold remembers its split under.
    #[serde(default)]
    pub project_root: String,
    #[serde(default)]
    pub clients: Vec<RemoteClientEditors>,
    /// Session documents the backend-local `FileEditorManager` selects.
    #[serde(default)]
    pub focused: Vec<String>,
}

/// One editor-surface observation as IntelliJ reported it. Every field is
/// optional on the wire: after a hot reload this library is newer than the
/// plugin generation calling it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceProjectionEvidence {
    /// The IDE project's base path.
    #[serde(default)]
    pub base_path: Option<String>,
    /// Every backend `EditorWindow`, across frames.
    #[serde(default)]
    pub backend_window_count: usize,
    /// The editor windows of the surface's frame: origin (when measured),
    /// selected tab, and session-document tabs.
    #[serde(default)]
    pub windows: Vec<EditorWindowEvidence>,
    /// True when two or more windows exist but their frame offered nothing to
    /// measure them against, so no local layout can be derived.
    #[serde(default)]
    pub window_geometry_unavailable: bool,
    /// The IDE selection history (least recently used first), restricted to
    /// the windows' session tabs.
    #[serde(default)]
    pub history: Vec<String>,
    /// `FileEditorManager.selectedFiles`, any file kind.
    #[serde(default)]
    pub manager_selected: Vec<String>,
    /// Every open session document of the project.
    #[serde(default)]
    pub open_session_documents: Vec<String>,
    #[serde(default)]
    pub remote: Option<RemoteSurfaceEvidence>,
    /// A JetBrains Client forwards measured columns for this project (GH #320
    /// option B), so its frontend owns the Remote Dev layout.
    #[serde(default)]
    pub measured_layout_authority: bool,
    /// The anchor window's selected tab, when it is a session document.
    #[serde(default)]
    pub anchor_selected_session: Option<String>,
    /// The selected text editor's file, when it is a session document.
    #[serde(default)]
    pub selected_text_editor_session: Option<String>,
    /// The document the triggering event selected.
    #[serde(default)]
    pub preferred: Option<String>,
    /// Whether [`Self::preferred`] is a valid session document.
    #[serde(default)]
    pub preferred_session_document: bool,
    /// The document the triggering selection event left.
    #[serde(default)]
    pub previous: Option<String>,
    /// The observation authority (`document_selection`, `layout`, ...).
    #[serde(default)]
    pub authority: Option<String>,
    /// Settling passes left before the selection edge applies authoritatively.
    #[serde(default)]
    pub remaining_selection_passes: Option<u32>,
    #[serde(default)]
    pub force_reconcile: bool,
    #[serde(default)]
    pub preserve_focus: bool,
}

/// Whether the projection may be published now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceProjectionOutcome {
    Ready,
    /// The editor projection has not settled; re-read on a later EDT turn.
    #[default]
    Deferred,
}

/// Which layout evidence the projection used.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceLayoutSource {
    #[default]
    None,
    Windows,
    Remote,
}

/// The derived surface and what to log about it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceProjection {
    pub outcome: SurfaceProjectionOutcome,
    /// Why the projection was deferred (`[layout-sync] capture deferred reason=`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The payload for the Project Controller. Columns are absolute for a
    /// local layout and project relative for a Remote Dev one; the plugin's
    /// root choice normalizes both against the surface root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<EditorSurface>,
    #[serde(default)]
    pub layout_source: SurfaceLayoutSource,
    /// A `[layout-detect]` observation line, deduplicated by the plugin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_line: Option<String>,
    /// Lines the plugin logs at INFO.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub info: Vec<String>,
    /// Lines the plugin logs at DEBUG.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub debug: Vec<String>,
}

impl SurfaceProjection {
    fn deferred(reason: impl Into<String>) -> Self {
        Self {
            outcome: SurfaceProjectionOutcome::Deferred,
            reason: Some(reason.into()),
            ..Self::default()
        }
    }
}

/// A resolved split layout: columns and their order provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Layout {
    columns: Vec<SurfaceColumn>,
    column_order: SurfaceColumnOrder,
}

fn distinct<'a>(items: impl IntoIterator<Item = &'a String>) -> Vec<String> {
    let mut seen = HashSet::new();
    items
        .into_iter()
        .filter(|item| seen.insert(item.as_str()))
        .cloned()
        .collect()
}

fn non_blank(value: &Option<String>) -> Option<&str> {
    value.as_deref().filter(|value| !value.trim().is_empty())
}

/// Whether `path` is absolute on any platform the IDE runs on.
pub(crate) fn is_absolute_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    path.starts_with('/')
        || path.starts_with('\\')
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'/' | b'\\'))
}

/// `SyncLayoutAction.absolutizeEditorLayout`: resolve relative columns against
/// `root`, dropping blanks; a layout whose columns are all empty is no layout.
fn absolutize(root: &str, layout: Layout) -> Option<Layout> {
    let root = root.trim_end_matches('/');
    let columns: Vec<SurfaceColumn> = layout
        .columns
        .into_iter()
        .map(|column| {
            SurfaceColumn::new(column.files.into_iter().filter_map(|file| {
                if file.trim().is_empty() {
                    None
                } else if is_absolute_path(&file) {
                    Some(file)
                } else {
                    Some(format!("{root}/{file}"))
                }
            }))
        })
        .collect();
    columns
        .iter()
        .any(|column| !column.files.is_empty())
        .then_some(Layout {
            columns,
            column_order: layout.column_order,
        })
}

/// Stale-selection settling (`SelectionProjectionSettling.reconcileEventEdge`):
/// once every settling pass is spent, the old document of a selection edge
/// that still shows where the new one should be is replaced by the new one.
fn replace_previous_if_stale(
    preferred: Option<&str>,
    previous: Option<&str>,
    files: Vec<String>,
) -> Vec<String> {
    let (Some(preferred), Some(previous)) = (
        preferred.filter(|path| !path.trim().is_empty()),
        previous.filter(|path| !path.trim().is_empty()),
    ) else {
        return files;
    };
    if files.iter().any(|file| file == preferred) || !files.iter().any(|file| file == previous) {
        return files;
    }
    let replaced: Vec<String> = files
        .into_iter()
        .map(|file| {
            if file == previous {
                preferred.to_string()
            } else {
                file
            }
        })
        .collect();
    distinct(replaced.iter())
}

/// `#stickymdpane`: the session document each restored window stands for.
fn visible_from_windows(
    selected: &[Option<String>],
    sticky: &[Option<String>],
    session_paths: &HashSet<&str>,
) -> Vec<String> {
    let in_session = |path: &&String| session_paths.contains(path.as_str());
    let picked: Vec<String> = selected
        .iter()
        .enumerate()
        .filter_map(|(index, selected)| {
            selected
                .as_ref()
                .filter(in_session)
                .or_else(|| {
                    sticky
                        .get(index)
                        .and_then(Option::as_ref)
                        .filter(in_session)
                })
                .cloned()
        })
        .collect();
    distinct(picked.iter())
}

/// GH #320: on a Remote Dev backend the clients' visible and selected session
/// documents join the backend's own selection.
fn remote_client_visible(
    backend_visible: &[String],
    clients: &[RemoteClientEditors],
    open_session_documents: &[String],
    base_path: Option<&str>,
) -> Vec<String> {
    let absolute = |relative: &String| -> Option<String> {
        // The last open document with that relative path wins, as Kotlin's `associate`.
        open_session_documents
            .iter()
            .rev()
            .find(|path| &crate::window_layout::project_relative_path(base_path, path) == relative)
            .cloned()
    };
    let joined: Vec<String> = backend_visible
        .iter()
        .cloned()
        .chain(
            clients
                .iter()
                .flat_map(|client| client.visible.iter())
                .filter_map(absolute),
        )
        .chain(
            clients
                .iter()
                .flat_map(|client| client.selected.iter())
                .filter_map(absolute),
        )
        .filter(|path| !path.trim().is_empty())
        .collect();
    distinct(joined.iter())
}

/// Open tabs ordered by distance from the focused tab, ties to the left.
pub fn tabs_by_proximity(focused: &str, tabs: &[String]) -> Vec<String> {
    let focused_index = tabs.iter().position(|tab| tab == focused).unwrap_or(0);
    let mut ranked: Vec<(usize, &String)> = tabs.iter().enumerate().collect();
    ranked.sort_by_key(|(index, _)| (index.abs_diff(focused_index), *index));
    distinct(ranked.into_iter().map(|(_, tab)| tab))
}

/// Focused first, then nearby tabs, then visible, then every open document.
pub fn prioritize_open_documents(
    focused: &str,
    nearby: &[String],
    visible: &[String],
    open: &[String],
) -> Vec<String> {
    let focused = focused.to_string();
    let all: Vec<&String> = std::iter::once(&focused)
        .chain(nearby)
        .chain(visible)
        .chain(open)
        .filter(|path| !path.trim().is_empty())
        .collect();
    distinct(all)
}

fn render_files(files: &[String]) -> String {
    if files.is_empty() {
        "<none>".to_string()
    } else {
        files.join(",")
    }
}

fn render_per_client(per_client: &[&[String]]) -> String {
    per_client
        .iter()
        .enumerate()
        .map(|(index, files)| format!("{index}:[{}]", render_files(files)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// GH #175: the raw Remote Dev evidence an observed line carries.
fn remote_client_evidence_line(clients: &[RemoteClientEditors]) -> String {
    let rendered = clients
        .iter()
        .enumerate()
        .map(|(index, client)| {
            format!(
                "{index}:{{visible=[{}] selected=[{}] open=[{}]}}",
                render_files(&client.visible),
                render_files(&client.selected),
                render_files(&client.open)
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    format!("remote_clients={} clients=[{rendered}]", clients.len())
}

fn unknown_remote_layout_line(remote: &RemoteSurfaceEvidence, reason: &str) -> String {
    let selections: Vec<&[String]> = remote
        .clients
        .iter()
        .map(|client| client.selected.as_slice())
        .collect();
    let visible: Vec<&[String]> = remote
        .clients
        .iter()
        .map(|client| client.visible.as_slice())
        .collect();
    let visible = if visible.is_empty() {
        String::new()
    } else {
        format!("visible=[{}] ", render_per_client(&visible))
    };
    format!(
        "[layout-detect] unknown windows=0 source=remote_client_selected_files focused=[{}] \
         remote_clients={} selections=[{}] {visible}reason={reason}",
        render_files(&remote.focused),
        selections.len(),
        render_per_client(&selections),
    )
}

/// Resolve the Remote Dev layout through the fold and describe it.
fn remote_layout(
    remote: &RemoteSurfaceEvidence,
    resolution: RemoteLayoutResolution,
    projection: &mut SurfaceProjection,
) -> Option<Layout> {
    let columns: Vec<SurfaceColumn> = resolution
        .columns
        .iter()
        .map(|column| {
            SurfaceColumn::new(
                column
                    .files
                    .iter()
                    .filter(|file| !file.trim().is_empty())
                    .cloned(),
            )
        })
        .filter(|column| !column.files.is_empty())
        .collect();
    if columns.is_empty() {
        let reason = resolution
            .reason
            .as_deref()
            .unwrap_or("no_unique_client_split_set");
        projection.observed_line = Some(unknown_remote_layout_line(remote, reason));
        return None;
    }
    let snapshots: Vec<WindowSnapshot> = columns
        .iter()
        .flat_map(|column| column.files.iter())
        .map(|file| WindowSnapshot {
            x: 0,
            y: 0,
            file: Some(file.clone()),
        })
        .collect();
    let source = match &resolution.reason {
        Some(reason) => format!("{} reason={reason}", resolution.source.as_str()),
        None => resolution.source.as_str().to_string(),
    };
    projection.observed_line = Some(format!(
        "{} {}",
        observed_layout_line(0, &snapshots, &columns, &source),
        remote_client_evidence_line(&remote.clients)
    ));
    let column_order = match resolution.column_order {
        crate::remote_layout::RemoteLayoutColumnOrder::Unknown => SurfaceColumnOrder::Unknown,
        crate::remote_layout::RemoteLayoutColumnOrder::Retained => SurfaceColumnOrder::Retained,
    };
    Some(Layout {
        columns,
        column_order,
    })
}

/// Resolve the classic local window layout and describe it.
fn window_layout(
    evidence: &SurfaceProjectionEvidence,
    projection: &mut SurfaceProjection,
) -> Option<Layout> {
    let resolution = resolve_window_layout(&WindowLayoutEvidence {
        base_path: evidence.base_path.clone(),
        windows: evidence.windows.clone(),
        history: evidence.history.clone(),
    });
    if let Some(diagnostic) = resolution.diagnostic {
        projection.info.push(diagnostic);
    }
    projection.observed_line = resolution.observed_line;
    let columns: Vec<SurfaceColumn> = resolution
        .columns
        .into_iter()
        .map(|column| {
            SurfaceColumn::new(
                column
                    .files
                    .into_iter()
                    .filter(|file| !file.trim().is_empty()),
            )
        })
        .collect();
    if columns.is_empty() {
        projection.debug.push(format!(
            "[layout-detect] no layout reason={}",
            resolution.reason.as_deref().unwrap_or("<none>")
        ));
        return None;
    }
    let layout = Layout {
        columns,
        column_order: resolution.column_order,
    };
    match evidence.base_path.as_deref() {
        Some(base) => absolutize(base, layout),
        None => Some(layout),
    }
}

/// Derive the editor surface from one observation.
///
/// `remote_fold` answers the Remote Dev fold for `(project_root, evidence)`;
/// it is called at most once, and only for a ready projection whose layout is
/// the Remote Dev one.
pub fn project_surface(
    evidence: &SurfaceProjectionEvidence,
    remote_fold: impl FnOnce(&str, &RemoteLayoutEvidence) -> RemoteLayoutResolution,
) -> SurfaceProjection {
    let empty_remote = RemoteSurfaceEvidence::default();
    let remote = evidence.remote.as_ref().unwrap_or(&empty_remote);
    let windows = &evidence.windows;
    // GH #154 / #177: remote client sessions outrank incidental backend windows.
    let use_remote = !remote.clients.is_empty();
    // GH #320 option B: the client's measured columns own the Remote Dev layout.
    if use_remote && evidence.measured_layout_authority {
        return SurfaceProjection::deferred("frontend_measured_layout_authority");
    }

    let surface_session_tabs = distinct(windows.iter().flat_map(|window| &window.session_tabs));
    let open_session: Vec<String> = if use_remote {
        evidence.open_session_documents.clone()
    } else {
        surface_session_tabs
    };
    let session_paths: HashSet<&str> = open_session.iter().map(String::as_str).collect();

    // GH #88: a backend without editor windows names one selection per split.
    let selected: Vec<Option<String>> = if use_remote || windows.is_empty() {
        evidence
            .manager_selected
            .iter()
            .cloned()
            .map(Some)
            .collect()
    } else {
        windows
            .iter()
            .map(|window| window.selected.clone())
            .collect()
    };
    // GH #177: only after the remote-client authority decision does a backend
    // window without a selection gate the projection.
    let windows_ready = !selected.is_empty() && selected.iter().all(Option::is_some);
    if !use_remote && !windows_ready {
        return SurfaceProjection::deferred(format!(
            "backend_window_selection_incomplete windows={} selected={}",
            windows.len(),
            selected.len()
        ));
    }

    let sticky: Vec<Option<String>> = windows
        .iter()
        .map(|window| {
            let tabs: Vec<String> = window
                .session_tabs
                .iter()
                .filter(|path| session_paths.contains(path.as_str()))
                .cloned()
                .collect();
            sticky_session_document(
                window.selected.as_deref(),
                &session_tabs_mru_last(&tabs, &evidence.history),
            )
        })
        .collect();
    let window_visible = visible_from_windows(&selected, &sticky, &session_paths);
    let raw_visible = if use_remote {
        remote_client_visible(
            &window_visible,
            &remote.clients,
            &evidence.open_session_documents,
            evidence.base_path.as_deref(),
        )
    } else {
        window_visible
    };

    let reconcile = evidence.authority.as_deref() == Some("document_selection")
        && evidence.remaining_selection_passes == Some(0);
    let preferred_raw = evidence.preferred.as_deref();
    let previous_raw = evidence.previous.as_deref();
    let visible = if reconcile {
        replace_previous_if_stale(preferred_raw, previous_raw, raw_visible)
    } else {
        raw_visible
    };
    if visible.is_empty() {
        return SurfaceProjection::deferred("no_visible_session_documents");
    }

    let preferred = preferred_raw
        .filter(|path| evidence.preferred_session_document && session_paths.contains(path));
    if preferred_raw.is_some() && preferred.is_none() {
        return SurfaceProjection::deferred("preferred_file_not_open_session_document");
    }
    if let Some(preferred) = preferred.filter(|path| !path.trim().is_empty())
        && !visible.iter().any(|file| file == preferred)
    {
        return SurfaceProjection::deferred("preferred_document_not_in_current_projection");
    }

    let selected_editor = if use_remote {
        &evidence.selected_text_editor_session
    } else {
        &evidence.anchor_selected_session
    };
    let Some(active) = preferred
        .filter(|path| !path.trim().is_empty())
        .or_else(|| non_blank(selected_editor))
        .or_else(|| visible.first().map(String::as_str))
        .map(str::to_string)
    else {
        return SurfaceProjection::deferred("active_session_document_unresolved");
    };
    let resolvable = preferred == Some(active.as_str())
        || selected_editor.as_deref() == Some(active.as_str())
        || session_paths.contains(active.as_str());
    if !resolvable {
        return SurfaceProjection::deferred("active_virtual_file_unresolved");
    }

    let focused_window_tabs: &[String] = windows
        .iter()
        .find(|window| window.selected.as_deref() == Some(active.as_str()))
        .map(|window| window.session_tabs.as_slice())
        .unwrap_or_default();
    let open = prioritize_open_documents(
        &active,
        &tabs_by_proximity(&active, focused_window_tabs),
        &visible,
        &open_session,
    );

    let mut projection = SurfaceProjection {
        outcome: SurfaceProjectionOutcome::Ready,
        ..SurfaceProjection::default()
    };
    // GH #157 / #326: the layout resolves only for a ready projection, so a
    // deferred capture never advances the Remote Dev fold's memory.
    let uses_remote_layout = use_remote || windows.is_empty();
    let local = !uses_remote_layout && !evidence.window_geometry_unavailable;
    let layout = if uses_remote_layout {
        projection.layout_source = SurfaceLayoutSource::Remote;
        let resolution = remote_fold(
            &remote.project_root,
            &RemoteLayoutEvidence {
                clients: remote.clients.clone(),
                focused: remote.focused.clone(),
            },
        );
        remote_layout(remote, resolution, &mut projection)
    } else if local {
        projection.layout_source = SurfaceLayoutSource::Windows;
        window_layout(evidence, &mut projection)
    } else {
        None
    };
    let layout = layout.map(|layout| {
        if !reconcile {
            return layout;
        }
        Layout {
            columns: layout
                .columns
                .into_iter()
                .map(|column| {
                    SurfaceColumn::new(replace_previous_if_stale(
                        preferred_raw,
                        previous_raw,
                        column.files,
                    ))
                })
                .collect(),
            column_order: layout.column_order,
        }
    });
    // The capture's readiness half for the local layout: a layout that omits
    // the preferred document is withheld (unknown columns carry no structural
    // authority) rather than published stale.
    let layout = match layout {
        Some(layout)
            if local
                && preferred_raw.is_some_and(|preferred| {
                    !preferred.trim().is_empty()
                        && !layout
                            .columns
                            .iter()
                            .any(|column| column.files.iter().any(|file| file == preferred))
                }) =>
        {
            projection.info.push(format!(
                "[layout-sync] layout withheld reason=preferred_document_not_in_layout preferred={}",
                preferred_raw.unwrap_or_default()
            ));
            None
        }
        layout => layout,
    };

    let columns: Vec<SurfaceColumn> = layout
        .as_ref()
        .map(|layout| {
            layout
                .columns
                .iter()
                .map(|column| {
                    SurfaceColumn::new(distinct(
                        column.files.iter().filter(|file| !file.trim().is_empty()),
                    ))
                })
                .filter(|column| !column.files.is_empty())
                .collect()
        })
        .unwrap_or_default();
    projection.surface = Some(EditorSurface {
        open: prioritize_open_documents(&active, &open, &visible, &[]),
        visible: distinct(visible.iter()),
        focused: active,
        columns,
        column_order: layout
            .map(|layout| layout.column_order)
            .unwrap_or(SurfaceColumnOrder::Editor),
        force_reconcile: evidence.force_reconcile,
        focus_only: false,
        preserve_focus: evidence.preserve_focus,
    });
    projection
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(selected: &str, tabs: &[&str], x: Option<i32>) -> EditorWindowEvidence {
        EditorWindowEvidence {
            x,
            y: x.map(|_| 0),
            selected: Some(selected.to_string()),
            session_tabs: tabs.iter().map(|tab| tab.to_string()).collect(),
        }
    }

    fn no_fold(_: &str, _: &RemoteLayoutEvidence) -> RemoteLayoutResolution {
        panic!("the remote fold must not run")
    }

    fn retained_fold(_: &str, _: &RemoteLayoutEvidence) -> RemoteLayoutResolution {
        RemoteLayoutResolution {
            columns: vec![SurfaceColumn::new(["c.md"]), SurfaceColumn::new(["b.md"])],
            source: crate::RemoteLayoutSource::RetainedRemoteColumns,
            column_order: crate::remote_layout::RemoteLayoutColumnOrder::Retained,
            reason: Some("selected_visible_change_preserved_structure".into()),
        }
    }

    #[test]
    fn a_two_window_split_projects_absolute_columns_and_proximity_order() {
        let evidence = SurfaceProjectionEvidence {
            base_path: Some("/repo".into()),
            backend_window_count: 2,
            windows: vec![
                window("/repo/a.md", &["/repo/a.md", "/repo/c.md"], Some(0)),
                window("/repo/b.md", &["/repo/b.md"], Some(640)),
            ],
            anchor_selected_session: Some("/repo/b.md".into()),
            preferred: Some("/repo/b.md".into()),
            preferred_session_document: true,
            authority: Some("document_selection".into()),
            remaining_selection_passes: Some(3),
            ..Default::default()
        };
        let projection = project_surface(&evidence, no_fold);
        assert_eq!(projection.outcome, SurfaceProjectionOutcome::Ready);
        assert_eq!(projection.layout_source, SurfaceLayoutSource::Windows);
        let surface = projection.surface.unwrap();
        assert_eq!(surface.focused, "/repo/b.md");
        assert_eq!(surface.visible, ["/repo/a.md", "/repo/b.md"]);
        assert_eq!(surface.open, ["/repo/b.md", "/repo/a.md", "/repo/c.md"]);
        assert_eq!(
            surface.columns,
            [
                SurfaceColumn::new(["/repo/a.md"]),
                SurfaceColumn::new(["/repo/b.md"])
            ]
        );
        assert_eq!(surface.column_order, SurfaceColumnOrder::Editor);
    }

    #[test]
    fn a_deferred_projection_never_advances_the_remote_fold() {
        let evidence = SurfaceProjectionEvidence {
            remote: Some(RemoteSurfaceEvidence {
                project_root: "/repo".into(),
                clients: vec![RemoteClientEditors::default()],
                focused: vec![],
            }),
            ..Default::default()
        };
        let projection = project_surface(&evidence, no_fold);
        assert_eq!(projection.outcome, SurfaceProjectionOutcome::Deferred);
        assert_eq!(
            projection.reason.as_deref(),
            Some("no_visible_session_documents")
        );
    }

    #[test]
    fn a_remote_retained_fold_reaches_the_surface_unchanged() {
        let evidence = SurfaceProjectionEvidence {
            base_path: Some("/repo".into()),
            manager_selected: vec!["/repo/c.md".into()],
            open_session_documents: ["/repo/a.md", "/repo/b.md", "/repo/c.md"]
                .map(String::from)
                .to_vec(),
            remote: Some(RemoteSurfaceEvidence {
                project_root: "/repo".into(),
                clients: vec![RemoteClientEditors {
                    visible: vec!["a.md".into(), "b.md".into()],
                    selected: vec!["c.md".into()],
                    open: vec!["a.md".into(), "b.md".into(), "c.md".into()],
                }],
                focused: vec!["c.md".into()],
            }),
            selected_text_editor_session: Some("/repo/c.md".into()),
            ..Default::default()
        };
        let projection = project_surface(&evidence, retained_fold);
        assert_eq!(projection.outcome, SurfaceProjectionOutcome::Ready);
        assert_eq!(projection.layout_source, SurfaceLayoutSource::Remote);
        assert_eq!(
            projection.surface.unwrap().column_order,
            SurfaceColumnOrder::Retained
        );
    }

    #[test]
    fn tabs_rank_by_distance_from_the_focused_tab() {
        let tabs: Vec<String> = ["a", "b", "c", "d", "e"].map(String::from).to_vec();
        assert_eq!(tabs_by_proximity("c", &tabs), ["c", "b", "d", "a", "e"]);
        assert_eq!(tabs_by_proximity("z", &tabs), ["a", "b", "c", "d", "e"]);
    }

    #[test]
    fn absolute_paths_cover_posix_and_drive_letters() {
        assert!(is_absolute_path("/repo/a.md"));
        assert!(is_absolute_path("C:/repo/a.md"));
        assert!(!is_absolute_path("tasks/a.md"));
    }
}
