//! Editor-window layout derivation behind the hot-reloaded native boundary
//! (GH #326, the #295 "path 2" lever).
//!
//! A JetBrains plugin generation can only be replaced by an IDE restart on the
//! exact-263 modular package (GH #299), while `libagent_doc` hot-reloads under
//! the running IDE. Every rule that turns editor-window state into a split
//! layout therefore lives here, and the plugin only reports what each editor
//! window *is*: where it sits, which tab it selects, and which session
//! documents it holds. Changing how that evidence becomes columns, which
//! window stands for which document, or how order provenance is labelled is a
//! native change that ships without a plugin generation.
//!
//! Two derivations share the column grouping:
//!
//! - [`resolve_window_layout`]: the classic local `EditorWindow` path
//!   (`LayoutDetector.detectEditorLayout`). Windows that share an origin were
//!   never laid out (GH #77), so each stays its own column and the order is
//!   `unknown` (GH #320); a window showing a source tab still stands for the
//!   last session document it showed (`#stickymdpane`); one window is a known
//!   single-column layout, never `unknown`.
//! - [`resolve_client_measured_layout`]: the JetBrains Client's measured main
//!   window forwarded by the modular backend (GH #320 option B). Only measured,
//!   distinct origins are authority; anything less keeps the last forwarded
//!   layout in force instead of collapsing it.
//!
//! Both are pure total functions over plain values. The plugin keeps a frozen
//! compatibility copy only for a native library that cannot answer; new rules
//! land here, never there.

use serde::{Deserialize, Serialize};

use crate::{SurfaceColumn, SurfaceColumnOrder};

/// Windows whose left edges differ by at most this many pixels are one column.
pub const COLUMN_X_TOLERANCE_PX: i32 = 8;

/// What one local editor window reports. Paths are the editor's absolute paths.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorWindowEvidence {
    /// Left edge in the frame's splitter coordinates; `None` when the window's
    /// bounds could not be read (a headless or remote backend component).
    #[serde(default)]
    pub x: Option<i32>,
    #[serde(default)]
    pub y: Option<i32>,
    /// The window's selected tab, whatever kind of file it is.
    #[serde(default)]
    pub selected: Option<String>,
    /// The window's tabs that are agent-doc session documents, in tab order.
    #[serde(default)]
    pub session_tabs: Vec<String>,
}

/// One observation of the local editor windows of the frame being mirrored.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowLayoutEvidence {
    /// The IDE project's base path; published columns are relative to it.
    #[serde(default)]
    pub base_path: Option<String>,
    #[serde(default)]
    pub windows: Vec<EditorWindowEvidence>,
    /// The IDE's selection history (least recently used first), restricted to
    /// the reported session tabs.
    #[serde(default)]
    pub history: Vec<String>,
}

/// One window as the grouping saw it: its origin and the document it stands for.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowSnapshot {
    pub x: i32,
    pub y: i32,
    #[serde(default)]
    pub file: Option<String>,
}

/// The layout a local window observation implies, and what to log about it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowLayoutResolution {
    /// Columns in left-to-right order, project-relative. Empty means "no
    /// layout" (structurally unknown). A known layout may contain an empty
    /// column: a split that shows no session document keeps its pane position.
    pub columns: Vec<SurfaceColumn>,
    pub column_order: SurfaceColumnOrder,
    /// Why there is no layout, when `columns` is empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The per-window origins and documents the grouping used.
    #[serde(default)]
    pub snapshots: Vec<WindowSnapshot>,
    /// The `[layout-detect] observed ...` line (GH #81), deduplicated by the plugin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_line: Option<String>,
    /// An info-level diagnostic about this observation, logged as is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<String>,
}

/// One JetBrains Client editor window of the main frame (GH #320 option B).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientWindowEvidence {
    /// The frontend's window ordinal; windows are considered in this order.
    #[serde(default)]
    pub ordinal: i64,
    /// The selected tab when it is a session document (absolute path).
    #[serde(default)]
    pub selected_session: Option<String>,
    /// Whether any open tab of this window is a session document.
    #[serde(default)]
    pub holds_session_document: bool,
    /// Measured left/top edge; `None` when the window was not showing.
    #[serde(default)]
    pub x: Option<i32>,
    #[serde(default)]
    pub y: Option<i32>,
}

/// The main-frame windows of one frontend surface snapshot.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientMeasuredLayoutEvidence {
    #[serde(default)]
    pub windows: Vec<ClientWindowEvidence>,
}

/// A measured main-window layout, or why the last forwarded one stays in force.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ClientMeasuredLayout {
    /// Columns (absolute paths) in measured left-to-right order, plus the
    /// selected document of the first measured window.
    Measured {
        columns: Vec<Vec<String>>,
        focused: String,
    },
    Unavailable {
        reason: String,
    },
}

/// `#stickymdpane`: the document a window stands for. The selected tab when it
/// is one of the window's documents, else the most recently used document of
/// that window, else nothing.
pub fn sticky_session_document(
    selected: Option<&str>,
    session_tabs_mru_last: &[String],
) -> Option<String> {
    selected
        .filter(|path| session_tabs_mru_last.iter().any(|tab| tab == path))
        .map(str::to_string)
        .or_else(|| session_tabs_mru_last.last().cloned())
}

/// A window's markdown session tabs ordered so the most recently used is last.
/// Tabs the history never saw keep tab order, ahead of every tab it has seen.
pub fn session_tabs_mru_last(session_tabs: &[String], history: &[String]) -> Vec<String> {
    let rank = |path: &String| -> i64 {
        history
            .iter()
            .position(|seen| seen == path)
            .map_or(-1, |index| index as i64)
    };
    let mut tabs: Vec<String> = session_tabs
        .iter()
        .filter(|path| path.ends_with(".md"))
        .cloned()
        .collect();
    // Stable sort: equal ranks (never seen) keep tab order.
    tabs.sort_by_key(rank);
    tabs
}

/// `path` relative to `base_path` when it lies under it, else unchanged.
pub fn project_relative_path(base_path: Option<&str>, path: &str) -> String {
    let Some(base) = base_path else {
        return path.to_string();
    };
    if !path.starts_with(base) {
        return path.to_string();
    }
    path.strip_prefix(&format!("{base}/"))
        .unwrap_or(path)
        .to_string()
}

/// Two windows of a laid-out split never share an origin, so a shared origin
/// means the splitters were never laid out (GH #77).
pub fn shares_origin(snapshots: &[WindowSnapshot]) -> bool {
    snapshots.iter().enumerate().any(|(index, snapshot)| {
        snapshots[index + 1..]
            .iter()
            .any(|other| other.x == snapshot.x && other.y == snapshot.y)
    })
}

/// GH #320: only windows laid out at distinct origins are measured geometry.
pub fn measured_column_order(snapshots: &[WindowSnapshot]) -> SurfaceColumnOrder {
    if shares_origin(snapshots) {
        SurfaceColumnOrder::Unknown
    } else {
        SurfaceColumnOrder::Editor
    }
}

/// Group windows into columns by left edge, stacked top to bottom.
///
/// Windows that share an origin keep one column each, in window order: the
/// geometry is unavailable, and folding them by x would invent a vertical
/// stack the operator never made (GH #77).
pub fn build_columns(snapshots: &[WindowSnapshot], tolerance_px: i32) -> Vec<SurfaceColumn> {
    if snapshots.is_empty() {
        return Vec::new();
    }
    if shares_origin(snapshots) {
        return snapshots
            .iter()
            .map(|snapshot| SurfaceColumn::new(snapshot.file.iter().cloned()))
            .collect();
    }
    let mut sorted: Vec<&WindowSnapshot> = snapshots.iter().collect();
    sorted.sort_by_key(|snapshot| (snapshot.x, snapshot.y));
    let mut grouped: Vec<Vec<&WindowSnapshot>> = Vec::new();
    for snapshot in sorted {
        match grouped.last_mut() {
            Some(column) if (column[0].x - snapshot.x).abs() <= tolerance_px => {
                column.push(snapshot);
            }
            _ => grouped.push(vec![snapshot]),
        }
    }
    grouped
        .into_iter()
        .map(|mut column| {
            column.sort_by_key(|snapshot| snapshot.y);
            SurfaceColumn::new(
                column
                    .into_iter()
                    .filter_map(|snapshot| snapshot.file.clone()),
            )
        })
        .collect()
}

/// GH #81 discriminator: the window count, each window's origin and document,
/// and the columns built from them.
pub fn observed_layout_line(
    window_count: usize,
    snapshots: &[WindowSnapshot],
    columns: &[SurfaceColumn],
    source: &str,
) -> String {
    let snapshots = snapshots
        .iter()
        .map(|snapshot| {
            format!(
                "({},{}) {}",
                snapshot.x,
                snapshot.y,
                snapshot.file.as_deref().unwrap_or("<none>")
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let rendered = columns
        .iter()
        .map(|column| {
            if column.files.is_empty() {
                "<empty>".to_string()
            } else {
                column.files.join(",")
            }
        })
        .collect::<Vec<_>>()
        .join(" | ");
    format!(
        "[layout-detect] observed windows={window_count} snapshots=[{snapshots}] columns={} [{rendered}] source={source}",
        columns.len()
    )
}

/// Derive the local editor layout from one window observation.
pub fn resolve_window_layout(evidence: &WindowLayoutEvidence) -> WindowLayoutResolution {
    let base = evidence.base_path.as_deref();
    let windows = &evidence.windows;
    if windows.len() < 2 {
        // A proven single local window is a known one-column layout.
        let selected = windows.first().and_then(|window| {
            window
                .selected
                .as_deref()
                .filter(|path| window.session_tabs.iter().any(|tab| tab == path))
                .map(|path| project_relative_path(base, path))
        });
        let snapshots: Vec<WindowSnapshot> = windows
            .iter()
            .map(|_| WindowSnapshot {
                x: 0,
                y: 0,
                file: selected.clone(),
            })
            .collect();
        let columns: Vec<SurfaceColumn> = selected
            .iter()
            .map(|file| SurfaceColumn::new([file.clone()]))
            .collect();
        return WindowLayoutResolution {
            observed_line: Some(observed_layout_line(
                windows.len(),
                &snapshots,
                &columns,
                "windows",
            )),
            reason: columns
                .is_empty()
                .then(|| "no_selected_session_document".to_string()),
            columns,
            column_order: SurfaceColumnOrder::Editor,
            snapshots,
            diagnostic: None,
        };
    }

    // Any unmeasured window collapses every window onto one origin, which the
    // grouping reads as "keep each window its own column".
    let geometry_known = windows
        .iter()
        .all(|window| window.x.is_some() && window.y.is_some());
    let snapshots: Vec<WindowSnapshot> = windows
        .iter()
        .map(|window| {
            let tabs = session_tabs_mru_last(&window.session_tabs, &evidence.history);
            let file = sticky_session_document(window.selected.as_deref(), &tabs)
                .map(|path| project_relative_path(base, &path));
            WindowSnapshot {
                x: if geometry_known {
                    window.x.unwrap_or_default()
                } else {
                    0
                },
                y: if geometry_known {
                    window.y.unwrap_or_default()
                } else {
                    0
                },
                file,
            }
        })
        .collect();
    if snapshots.iter().all(|snapshot| snapshot.file.is_none()) {
        return WindowLayoutResolution {
            reason: Some("no_session_document_in_any_window".to_string()),
            snapshots,
            ..WindowLayoutResolution::default()
        };
    }
    let diagnostic = shares_origin(&snapshots).then(|| {
        format!(
            "[layout-detect] {} editor windows share an origin; geometry unavailable, keeping each window as its own column",
            windows.len()
        )
    });
    let columns = build_columns(&snapshots, COLUMN_X_TOLERANCE_PX);
    let observed_line = Some(observed_layout_line(
        windows.len(),
        &snapshots,
        &columns,
        "windows",
    ));
    // At least two columns (even if some are empty) are a layout; empty columns
    // tell sync to leave that tmux pane position alone.
    if columns.len() >= 2 {
        WindowLayoutResolution {
            column_order: measured_column_order(&snapshots),
            columns,
            reason: None,
            snapshots,
            observed_line,
            diagnostic,
        }
    } else {
        WindowLayoutResolution {
            columns: Vec::new(),
            column_order: SurfaceColumnOrder::Editor,
            reason: Some("fewer_than_two_columns".to_string()),
            snapshots,
            observed_line,
            diagnostic,
        }
    }
}

/// Derive the JetBrains Client's measured main-window layout (GH #320 option B).
pub fn resolve_client_measured_layout(
    evidence: &ClientMeasuredLayoutEvidence,
) -> ClientMeasuredLayout {
    let unavailable = |reason: &str| ClientMeasuredLayout::Unavailable {
        reason: reason.to_string(),
    };
    let mut windows: Vec<&ClientWindowEvidence> = evidence.windows.iter().collect();
    windows.sort_by_key(|window| window.ordinal);
    let mut measured = Vec::new();
    for window in windows {
        let Some(selected) = window.selected_session.as_ref() else {
            // `#stickymdpane`: a split showing a source tab next to its session
            // documents is still on screen; its column is unknown, not gone.
            // Keep the last forwarded layout rather than collapse it. A window
            // with no session document contributes no column, so closing a
            // split still collapses the layout.
            if window.holds_session_document {
                return unavailable("split_shows_non_session_tab");
            }
            continue;
        };
        let (Some(x), Some(y)) = (window.x, window.y) else {
            return unavailable("window_unmeasured");
        };
        measured.push(WindowSnapshot {
            x,
            y,
            file: Some(selected.clone()),
        });
    }
    if measured.is_empty() {
        return unavailable("no_session_documents");
    }
    if !measured_column_order(&measured).is_geometry() {
        return unavailable("shared_origin");
    }
    let columns: Vec<Vec<String>> = build_columns(&measured, COLUMN_X_TOLERANCE_PX)
        .into_iter()
        .map(|column| {
            let mut files: Vec<String> = Vec::new();
            for file in column.files {
                if !file.trim().is_empty() && !files.contains(&file) {
                    files.push(file);
                }
            }
            files
        })
        .filter(|files| !files.is_empty())
        .collect();
    if columns.is_empty() {
        return unavailable("no_session_documents");
    }
    let focused = measured
        .iter()
        .find_map(|snapshot| snapshot.file.clone())
        .unwrap_or_default();
    ClientMeasuredLayout::Measured { columns, focused }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(x: i32, y: i32, file: Option<&str>) -> WindowSnapshot {
        WindowSnapshot {
            x,
            y,
            file: file.map(str::to_string),
        }
    }

    fn window(
        x: Option<i32>,
        y: Option<i32>,
        selected: Option<&str>,
        tabs: &[&str],
    ) -> EditorWindowEvidence {
        EditorWindowEvidence {
            x,
            y,
            selected: selected.map(str::to_string),
            session_tabs: tabs.iter().map(|tab| tab.to_string()).collect(),
        }
    }

    fn files(columns: &[SurfaceColumn]) -> Vec<Vec<&str>> {
        columns
            .iter()
            .map(|column| column.files.iter().map(String::as_str).collect())
            .collect()
    }

    #[test]
    fn columns_follow_screen_order_when_the_focused_window_is_listed_first() {
        let columns = build_columns(
            &[
                snapshot(600, 0, Some("right.md")),
                snapshot(0, 0, Some("left.md")),
            ],
            COLUMN_X_TOLERANCE_PX,
        );
        assert_eq!(files(&columns), vec![vec!["left.md"], vec!["right.md"]]);
    }

    #[test]
    fn windows_in_one_column_stack_by_y() {
        let columns = build_columns(
            &[
                snapshot(0, 400, Some("bottom.md")),
                snapshot(0, 0, Some("top.md")),
            ],
            COLUMN_X_TOLERANCE_PX,
        );
        assert_eq!(files(&columns), vec![vec!["top.md", "bottom.md"]]);
    }

    #[test]
    fn windows_within_the_x_tolerance_merge_into_one_column() {
        let columns = build_columns(
            &[
                snapshot(0, 0, Some("a.md")),
                snapshot(6, 300, Some("b.md")),
                snapshot(600, 0, Some("c.md")),
            ],
            COLUMN_X_TOLERANCE_PX,
        );
        assert_eq!(files(&columns), vec![vec!["a.md", "b.md"], vec!["c.md"]]);
    }

    #[test]
    fn a_non_session_pane_keeps_an_empty_column() {
        let columns = build_columns(
            &[snapshot(0, 0, None), snapshot(600, 0, Some("right.md"))],
            COLUMN_X_TOLERANCE_PX,
        );
        assert_eq!(files(&columns), vec![vec![], vec!["right.md"]]);
    }

    #[test]
    fn unlaid_out_splits_stay_apart_in_window_order() {
        // GH #77: a Remote Dev backend reports every split at (0,0).
        let columns = build_columns(
            &[
                snapshot(0, 0, Some("tasks/agent-doc/agent-doc.md")),
                snapshot(0, 0, Some("tasks/pmt2/mr/1109.md")),
            ],
            COLUMN_X_TOLERANCE_PX,
        );
        assert_eq!(
            files(&columns),
            vec![
                vec!["tasks/agent-doc/agent-doc.md"],
                vec!["tasks/pmt2/mr/1109.md"]
            ]
        );
    }

    #[test]
    fn only_distinct_origins_are_measured_order() {
        assert_eq!(
            measured_column_order(&[snapshot(0, 0, Some("a.md")), snapshot(600, 0, Some("b.md"))]),
            SurfaceColumnOrder::Editor
        );
        assert_eq!(
            measured_column_order(&[snapshot(0, 0, Some("a.md")), snapshot(0, 0, Some("b.md"))]),
            SurfaceColumnOrder::Unknown
        );
    }

    #[test]
    fn sticky_prefers_the_live_selection_then_the_last_used_document() {
        let tabs = vec!["/r/stale.md".to_string(), "/r/now.md".to_string()];
        assert_eq!(
            sticky_session_document(Some("/r/now.md"), &tabs).as_deref(),
            Some("/r/now.md")
        );
        assert_eq!(
            sticky_session_document(Some("/r/src/Thing.kt"), &tabs).as_deref(),
            Some("/r/now.md")
        );
        assert_eq!(sticky_session_document(Some("/r/src/Thing.kt"), &[]), None);
    }

    #[test]
    fn mru_order_puts_unseen_tabs_first_in_tab_order() {
        let ordered = session_tabs_mru_last(
            &[
                "/r/a.md".to_string(),
                "/r/b.md".to_string(),
                "/r/c.md".to_string(),
                "/r/d.md".to_string(),
                "/r/Thing.kt".to_string(),
            ],
            &["/r/c.md".to_string(), "/r/a.md".to_string()],
        );
        assert_eq!(ordered, vec!["/r/b.md", "/r/d.md", "/r/c.md", "/r/a.md"]);
    }

    #[test]
    fn project_relative_paths_match_the_plugin_rule() {
        assert_eq!(
            project_relative_path(Some("/repo"), "/repo/tasks/a.md"),
            "tasks/a.md"
        );
        assert_eq!(
            project_relative_path(Some("/repo"), "/other/a.md"),
            "/other/a.md"
        );
        assert_eq!(
            project_relative_path(Some("/repo"), "/repository/a.md"),
            "/repository/a.md"
        );
        assert_eq!(project_relative_path(None, "/repo/a.md"), "/repo/a.md");
    }

    #[test]
    fn one_window_is_a_known_single_column_layout() {
        let resolution = resolve_window_layout(&WindowLayoutEvidence {
            base_path: Some("/repo".into()),
            windows: vec![window(
                Some(0),
                Some(0),
                Some("/repo/tasks/local.md"),
                &["/repo/tasks/local.md"],
            )],
            history: vec![],
        });
        assert_eq!(files(&resolution.columns), vec![vec!["tasks/local.md"]]);
        assert_eq!(resolution.column_order, SurfaceColumnOrder::Editor);
        assert_eq!(resolution.reason, None);
        assert_eq!(
            resolution.observed_line.as_deref(),
            Some(
                "[layout-detect] observed windows=1 snapshots=[(0,0) tasks/local.md] columns=1 [tasks/local.md] source=windows"
            )
        );
    }

    #[test]
    fn one_window_showing_source_is_no_layout() {
        let resolution = resolve_window_layout(&WindowLayoutEvidence {
            base_path: Some("/repo".into()),
            windows: vec![window(
                Some(0),
                Some(0),
                Some("/repo/src/Thing.kt"),
                &["/repo/tasks/local.md"],
            )],
            history: vec![],
        });
        assert!(resolution.columns.is_empty());
        assert_eq!(
            resolution.reason.as_deref(),
            Some("no_selected_session_document")
        );
    }

    #[test]
    fn two_measured_windows_publish_editor_order_and_keep_a_source_split_sticky() {
        let resolution = resolve_window_layout(&WindowLayoutEvidence {
            base_path: Some("/repo".into()),
            windows: vec![
                window(
                    Some(600),
                    Some(0),
                    Some("/repo/src/Thing.kt"),
                    &["/repo/tasks/older.md", "/repo/tasks/recent.md"],
                ),
                window(
                    Some(0),
                    Some(0),
                    Some("/repo/tasks/left.md"),
                    &["/repo/tasks/left.md"],
                ),
            ],
            history: vec![
                "/repo/tasks/older.md".into(),
                "/repo/tasks/recent.md".into(),
            ],
        });
        assert_eq!(
            files(&resolution.columns),
            vec![vec!["tasks/left.md"], vec!["tasks/recent.md"]]
        );
        assert_eq!(resolution.column_order, SurfaceColumnOrder::Editor);
        assert_eq!(resolution.diagnostic, None);
    }

    #[test]
    fn an_unmeasured_window_makes_the_whole_order_unknown() {
        // GH #81 + GH #320: one unreadable bound means geometry is unavailable
        // for the whole layout, never that the layout does not exist.
        let resolution = resolve_window_layout(&WindowLayoutEvidence {
            base_path: Some("/repo".into()),
            windows: vec![
                window(Some(600), Some(0), Some("/repo/b.md"), &["/repo/b.md"]),
                window(None, None, Some("/repo/a.md"), &["/repo/a.md"]),
            ],
            history: vec![],
        });
        assert_eq!(files(&resolution.columns), vec![vec!["b.md"], vec!["a.md"]]);
        assert_eq!(resolution.column_order, SurfaceColumnOrder::Unknown);
        assert!(
            resolution
                .diagnostic
                .as_deref()
                .is_some_and(|line| line.contains("share an origin"))
        );
    }

    #[test]
    fn windows_without_any_session_document_are_no_layout() {
        let resolution = resolve_window_layout(&WindowLayoutEvidence {
            base_path: Some("/repo".into()),
            windows: vec![
                window(Some(0), Some(0), Some("/repo/a.kt"), &[]),
                window(Some(600), Some(0), Some("/repo/b.kt"), &[]),
            ],
            history: vec![],
        });
        assert!(resolution.columns.is_empty());
        assert_eq!(resolution.observed_line, None);
        assert_eq!(
            resolution.reason.as_deref(),
            Some("no_session_document_in_any_window")
        );
    }

    #[test]
    fn stacked_windows_in_one_column_are_not_a_split() {
        let resolution = resolve_window_layout(&WindowLayoutEvidence {
            base_path: None,
            windows: vec![
                window(Some(0), Some(0), Some("/r/top.md"), &["/r/top.md"]),
                window(Some(0), Some(400), Some("/r/bottom.md"), &["/r/bottom.md"]),
            ],
            history: vec![],
        });
        assert!(resolution.columns.is_empty());
        assert_eq!(resolution.reason.as_deref(), Some("fewer_than_two_columns"));
        assert!(
            resolution
                .observed_line
                .as_deref()
                .is_some_and(|line| line.contains("columns=1 [/r/top.md,/r/bottom.md]"))
        );
    }

    #[test]
    fn observed_line_names_windows_origins_documents_and_columns() {
        let line = observed_layout_line(
            2,
            &[snapshot(0, 0, Some("a.md")), snapshot(0, 0, None)],
            &[SurfaceColumn::new(["a.md"]), SurfaceColumn::default()],
            "windows",
        );
        assert_eq!(
            line,
            "[layout-detect] observed windows=2 snapshots=[(0,0) a.md, (0,0) <none>] columns=2 [a.md | <empty>] source=windows"
        );
    }

    fn client(
        ordinal: i64,
        selected: Option<&str>,
        holds: bool,
        origin: Option<(i32, i32)>,
    ) -> ClientWindowEvidence {
        ClientWindowEvidence {
            ordinal,
            selected_session: selected.map(str::to_string),
            holds_session_document: holds,
            x: origin.map(|(x, _)| x),
            y: origin.map(|(_, y)| y),
        }
    }

    #[test]
    fn client_measured_columns_follow_geometry_not_ordinal() {
        let layout = resolve_client_measured_layout(&ClientMeasuredLayoutEvidence {
            windows: vec![
                client(0, Some("/r/right.md"), true, Some((800, 0))),
                client(1, Some("/r/left.md"), true, Some((0, 0))),
            ],
        });
        assert_eq!(
            layout,
            ClientMeasuredLayout::Measured {
                columns: vec![vec!["/r/left.md".into()], vec!["/r/right.md".into()]],
                focused: "/r/right.md".into(),
            }
        );
    }

    #[test]
    fn client_measured_layout_refuses_anything_short_of_measured_geometry() {
        let reason = |windows| match resolve_client_measured_layout(&ClientMeasuredLayoutEvidence {
            windows,
        }) {
            ClientMeasuredLayout::Unavailable { reason } => reason,
            other => panic!("expected unavailable, got {other:?}"),
        };
        assert_eq!(
            reason(vec![
                client(0, Some("/r/a.md"), true, Some((0, 0))),
                client(1, None, true, Some((800, 0))),
            ]),
            "split_shows_non_session_tab"
        );
        assert_eq!(
            reason(vec![client(0, Some("/r/a.md"), true, None)]),
            "window_unmeasured"
        );
        assert_eq!(
            reason(vec![
                client(0, Some("/r/a.md"), true, Some((0, 0))),
                client(1, Some("/r/b.md"), true, Some((0, 0))),
            ]),
            "shared_origin"
        );
        assert_eq!(
            reason(vec![client(0, None, false, Some((0, 0)))]),
            "no_session_documents"
        );
    }

    #[test]
    fn a_source_only_client_window_contributes_no_column() {
        let layout = resolve_client_measured_layout(&ClientMeasuredLayoutEvidence {
            windows: vec![
                client(0, Some("/r/a.md"), true, Some((0, 0))),
                client(1, None, false, Some((800, 0))),
            ],
        });
        assert_eq!(
            layout,
            ClientMeasuredLayout::Measured {
                columns: vec![vec!["/r/a.md".into()]],
                focused: "/r/a.md".into(),
            }
        );
    }

    #[test]
    fn evidence_without_optional_fields_still_parses() {
        // The plugin is older than the native library after a hot reload: every
        // evidence field is optional, so a newer library accepts older evidence.
        let evidence: WindowLayoutEvidence =
            serde_json::from_str(r#"{"windows":[{"selected":"/r/a.md"}],"future":1}"#).unwrap();
        assert_eq!(evidence.windows[0].selected.as_deref(), Some("/r/a.md"));
        assert!(evidence.windows[0].session_tabs.is_empty());
        let client: ClientMeasuredLayoutEvidence =
            serde_json::from_str(r#"{"windows":[{"ordinal":2}]}"#).unwrap();
        assert_eq!(client.windows[0].ordinal, 2);
        assert_eq!(
            serde_json::to_value(ClientMeasuredLayout::Unavailable {
                reason: "shared_origin".into()
            })
            .unwrap(),
            serde_json::json!({"outcome": "unavailable", "reason": "shared_origin"})
        );
    }
}
