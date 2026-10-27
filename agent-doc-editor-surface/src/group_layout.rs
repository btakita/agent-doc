//! VS Code editor-group layout derivation behind the hot-reloaded native
//! boundary (`#vscodeupgradelayout`, `editors/SPEC.md` § 13).
//!
//! The VS Code extension reports what its editor groups *are*: the grid that
//! `vscode.getEditorLayout` returns, how many tab groups exist, which document
//! each visible editor shows, and which group holds the focused document. Every
//! rule that turns that evidence into split columns, a split orientation, and a
//! claim position lives here, so changing a rule ships with `libagent_doc`
//! (which hot-reloads under a running extension host) instead of a VSIX, which
//! needs Restart Extensions locally and Reload Window on a remote host.
//!
//! The extension keeps a byte-for-byte TypeScript port (`groupLayout.ts`) as
//! the floor for a native library that cannot answer. Both answer every case of
//! `editors/vscode/test-fixtures/editor-derivations/editor_group_layout.json`;
//! a native rule change updates that file, and the floor may then diverge only
//! through a case's explicit `floor_expected`.
//!
//! # Geometry
//!
//! `vscode.getEditorLayout` returns the grid as a tree: `orientation` `0` lays
//! the root's groups out left to right and `1` top to bottom, each nested level
//! alternates, and `size` is a group's share along its parent's axis (pixels
//! from a live grid, any positive unit from `setEditorLayout`). Leaves in
//! depth-first order are the groups in VS Code's grid-appearance order, which
//! is also `viewColumn` order (`editorGroupToColumn`). With more than one
//! editor window (auxiliary windows), `getEditorLayout` describes only the
//! active window while tab groups span every window, so a leaf count that
//! differs from the tab-group count proves the geometry cannot be mapped onto
//! view columns and the column order is `unknown` (GH #320).
//!
//! Pure total functions over plain values.

use serde::{Deserialize, Serialize};

use crate::{SurfaceColumn, SurfaceColumnOrder};

/// Group left edges within this fraction of the editor area's width are one
/// column. Structurally aligned groups share an edge exactly; the tolerance
/// only joins groups the operator dragged to (almost) the same position.
pub const GROUP_COLUMN_X_TOLERANCE: f64 = 0.01;

/// One node of `vscode.getEditorLayout`'s grid (`GroupLayoutArgument`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EditorGroupLayoutNode {
    #[serde(default)]
    pub size: Option<f64>,
    /// Children of a branch; absent, `null`, or empty for a leaf (an editor
    /// group).
    #[serde(default)]
    pub groups: Option<Vec<EditorGroupLayoutNode>>,
}

impl EditorGroupLayoutNode {
    fn children(&self) -> &[EditorGroupLayoutNode] {
        self.groups.as_deref().unwrap_or_default()
    }
}

/// `vscode.getEditorLayout`'s answer (`EditorGroupLayout`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EditorGroupLayout {
    /// `0` horizontal (left to right), `1` vertical (top to bottom). Anything
    /// but `1` is read as horizontal, VS Code's default.
    #[serde(default)]
    pub orientation: Option<i64>,
    #[serde(default)]
    pub groups: Option<Vec<EditorGroupLayoutNode>>,
}

impl EditorGroupLayout {
    fn horizontal(&self) -> bool {
        self.orientation != Some(1)
    }
}

/// One visible text editor.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupEditorEvidence {
    /// The editor's `viewColumn` (1-based); `None` when VS Code reported none.
    #[serde(default)]
    pub view_column: Option<i64>,
    /// The document when it is a markdown document of the observed project, in
    /// the path form the caller publishes; `None` for any other editor, which
    /// still makes its group a column.
    #[serde(default)]
    pub file: Option<String>,
}

/// One observation of the VS Code editor groups.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GroupLayoutEvidence {
    /// `vscode.getEditorLayout`, or `None` when the command was unavailable.
    #[serde(default)]
    pub layout: Option<EditorGroupLayout>,
    /// `vscode.window.tabGroups.all.length`.
    #[serde(default)]
    pub group_count: Option<i64>,
    /// `vscode.window.visibleTextEditors`, in that order.
    #[serde(default)]
    pub editors: Vec<GroupEditorEvidence>,
    /// Index into `vscode.window.tabGroups.all` of the group holding the
    /// focused document, when one does.
    #[serde(default)]
    pub focused_group: Option<i64>,
}

/// The layout an editor-group observation implies.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupLayoutResolution {
    /// Columns in left-to-right order; each lists its documents top to bottom.
    /// Empty means no layout. A known layout may hold an empty column: a group
    /// that shows no project markdown keeps its pane position.
    pub columns: Vec<SurfaceColumn>,
    pub column_order: SurfaceColumnOrder,
    /// `measured` (geometry from `getEditorLayout`) or `ordinal` (`viewColumn`
    /// order only).
    pub source: String,
    /// The root split's measured orientation, `h` or `v`; `None` without two
    /// measured groups.
    #[serde(default)]
    pub orientation: Option<String>,
    /// The focused group's side for `claim --position`/`init-session
    /// --position` (`left`, `right`, `top`, `bottom`).
    #[serde(default)]
    pub position: Option<String>,
    /// Why the geometry was not used, when `source` is `ordinal`.
    #[serde(default)]
    pub reason: Option<String>,
    /// The `[layout-detect] vscode ...` diagnostic line.
    pub observed_line: String,
}

/// One measured group: its view column, rectangle origin, and root branch.
#[derive(Debug, Clone, Copy, PartialEq)]
struct PlacedGroup {
    x: f64,
    y: f64,
    root_child: usize,
}

/// Each child's weight along its parent's axis. Missing or non-positive sizes
/// take the mean of the valid ones, or `1` when none is valid.
fn child_weights(children: &[EditorGroupLayoutNode]) -> Vec<f64> {
    let valid = |size: Option<f64>| size.filter(|size| size.is_finite() && *size > 0.0);
    let mut present_sum = 0.0;
    let mut present_count = 0usize;
    for child in children {
        if let Some(size) = valid(child.size) {
            present_sum += size;
            present_count += 1;
        }
    }
    let fallback = if present_count == 0 {
        1.0
    } else {
        present_sum / present_count as f64
    };
    children
        .iter()
        .map(|child| valid(child.size).unwrap_or(fallback))
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn place_groups(
    children: &[EditorGroupLayoutNode],
    horizontal: bool,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    root_child: Option<usize>,
    placed: &mut Vec<PlacedGroup>,
) {
    let weights = child_weights(children);
    let mut total = 0.0;
    for weight in &weights {
        total += weight;
    }
    let mut offset = 0.0;
    for (index, child) in children.iter().enumerate() {
        let weight = weights[index];
        let (child_x, child_y, child_width, child_height) = if horizontal {
            (
                x + width * (offset / total),
                y,
                width * (weight / total),
                height,
            )
        } else {
            (
                x,
                y + height * (offset / total),
                width,
                height * (weight / total),
            )
        };
        offset += weight;
        let branch = root_child.unwrap_or(index);
        if child.children().is_empty() {
            placed.push(PlacedGroup {
                x: child_x,
                y: child_y,
                root_child: branch,
            });
        } else {
            place_groups(
                child.children(),
                !horizontal,
                child_x,
                child_y,
                child_width,
                child_height,
                Some(branch),
                placed,
            );
        }
    }
}

/// The groups in depth-first (view column) order, each with its origin in the
/// unit square of the editor area.
fn measure_groups(layout: &EditorGroupLayout) -> Vec<PlacedGroup> {
    let mut placed = Vec::new();
    place_groups(
        layout.groups.as_deref().unwrap_or_default(),
        layout.horizontal(),
        0.0,
        0.0,
        1.0,
        1.0,
        None,
        &mut placed,
    );
    placed
}

/// The distinct documents each view column `1..=count` shows, in editor order.
fn files_by_view_column(editors: &[GroupEditorEvidence], count: usize) -> Vec<Vec<String>> {
    let mut columns = vec![Vec::<String>::new(); count];
    for editor in editors {
        let Some(view_column) = editor.view_column else {
            continue;
        };
        if view_column < 1 || view_column as usize > count {
            continue;
        }
        let Some(file) = editor.file.as_ref().filter(|file| !file.is_empty()) else {
            continue;
        };
        let column = &mut columns[view_column as usize - 1];
        if !column.contains(file) {
            column.push(file.clone());
        }
    }
    columns
}

fn max_view_column(editors: &[GroupEditorEvidence]) -> usize {
    editors
        .iter()
        .filter_map(|editor| editor.view_column)
        .filter(|view_column| *view_column >= 1)
        .max()
        .unwrap_or(0) as usize
}

/// Group measured groups into columns by left edge, each stacked top to bottom.
fn measured_columns(placed: &[PlacedGroup], files: &[Vec<String>]) -> Vec<SurfaceColumn> {
    let mut order: Vec<usize> = (0..placed.len()).collect();
    order.sort_by(|left, right| {
        let (left, right) = (&placed[*left], &placed[*right]);
        left.x
            .partial_cmp(&right.x)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                left.y
                    .partial_cmp(&right.y)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });
    let mut grouped: Vec<Vec<usize>> = Vec::new();
    for index in order {
        match grouped.last_mut() {
            Some(column)
                if (placed[column[0]].x - placed[index].x).abs() <= GROUP_COLUMN_X_TOLERANCE =>
            {
                column.push(index);
            }
            _ => grouped.push(vec![index]),
        }
    }
    grouped
        .into_iter()
        .map(|mut column| {
            column.sort_by(|left, right| {
                placed[*left]
                    .y
                    .partial_cmp(&placed[*right].y)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let mut column_files: Vec<String> = Vec::new();
            for index in column {
                for file in &files[index] {
                    if !column_files.contains(file) {
                        column_files.push(file.clone());
                    }
                }
            }
            SurfaceColumn::new(column_files)
        })
        .collect()
}

/// The pre-measurement rule: index 0 is `left`, any later group `right`.
fn ordinal_position(evidence: &GroupLayoutEvidence) -> Option<String> {
    if evidence.group_count.unwrap_or(0) < 2 {
        return None;
    }
    match evidence.focused_group {
        Some(0) => Some("left".to_string()),
        Some(index) if index >= 1 => Some("right".to_string()),
        _ => None,
    }
}

fn observed_line(
    evidence: &GroupLayoutEvidence,
    leaves: Option<usize>,
    resolution: &GroupLayoutResolution,
) -> String {
    let rendered = resolution
        .columns
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
    let group_count = evidence
        .group_count
        .map_or_else(|| "unknown".to_string(), |count| count.to_string());
    let leaves = leaves.map_or_else(|| "none".to_string(), |count| count.to_string());
    let mut line = format!(
        "[layout-detect] vscode groups={group_count} layout_groups={leaves} source={} orientation={} columns={} [{rendered}] order={}",
        resolution.source,
        resolution.orientation.as_deref().unwrap_or("none"),
        resolution.columns.len(),
        resolution.column_order.as_str(),
    );
    if let Some(reason) = &resolution.reason {
        line.push_str(&format!(" reason={reason}"));
    }
    line
}

/// Derive the VS Code split layout from one editor-group observation.
pub fn resolve_group_layout(evidence: &GroupLayoutEvidence) -> GroupLayoutResolution {
    let max_column = max_view_column(&evidence.editors);
    let placed = evidence.layout.as_ref().map(measure_groups);
    let leaves = placed.as_ref().map(Vec::len);
    let geometry_reason = match (&placed, evidence.group_count) {
        (None, _) => Some("layout_unavailable"),
        (Some(placed), _) if placed.is_empty() => Some("layout_unavailable"),
        (Some(placed), Some(count)) if count >= 0 && placed.len() == count as usize => {
            (max_column > placed.len()).then_some("layout_misses_visible_column")
        }
        (Some(_), _) => Some("layout_group_count_mismatch"),
    };

    let mut resolution = match (geometry_reason, &placed) {
        (None, Some(placed)) => {
            let files = files_by_view_column(&evidence.editors, placed.len());
            let horizontal = evidence
                .layout
                .as_ref()
                .is_none_or(EditorGroupLayout::horizontal);
            let position = if placed.len() < 2 {
                None
            } else {
                evidence
                    .focused_group
                    .filter(|index| *index >= 0 && (*index as usize) < placed.len())
                    .map(|index| {
                        let first = placed[index as usize].root_child == 0;
                        match (horizontal, first) {
                            (true, true) => "left",
                            (true, false) => "right",
                            (false, true) => "top",
                            (false, false) => "bottom",
                        }
                        .to_string()
                    })
            };
            GroupLayoutResolution {
                columns: measured_columns(placed, &files),
                column_order: SurfaceColumnOrder::Editor,
                source: "measured".to_string(),
                orientation: (placed.len() >= 2)
                    .then(|| if horizontal { "h" } else { "v" }.to_string()),
                position,
                reason: None,
                observed_line: String::new(),
            }
        }
        (reason, _) => {
            let reason = reason.unwrap_or("layout_unavailable");
            let columns = files_by_view_column(&evidence.editors, max_column)
                .into_iter()
                .map(SurfaceColumn::new)
                .collect();
            GroupLayoutResolution {
                columns,
                // Without any geometry the ordinal order keeps its pre-measurement
                // `editor` meaning; geometry that contradicts the view columns
                // (another editor window) proves the order is not measured.
                column_order: if reason == "layout_unavailable" {
                    SurfaceColumnOrder::Editor
                } else {
                    SurfaceColumnOrder::Unknown
                },
                source: "ordinal".to_string(),
                orientation: None,
                position: ordinal_position(evidence),
                reason: Some(reason.to_string()),
                observed_line: String::new(),
            }
        }
    };
    resolution.observed_line = observed_line(evidence, leaves, &resolution);
    resolution
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(size: f64) -> EditorGroupLayoutNode {
        EditorGroupLayoutNode {
            size: Some(size),
            groups: None,
        }
    }

    fn editor(view_column: i64, file: &str) -> GroupEditorEvidence {
        GroupEditorEvidence {
            view_column: Some(view_column),
            file: Some(file.to_string()),
        }
    }

    #[test]
    fn a_vertical_split_is_one_stacked_column() {
        let resolution = resolve_group_layout(&GroupLayoutEvidence {
            layout: Some(EditorGroupLayout {
                orientation: Some(1),
                groups: Some(vec![leaf(400.0), leaf(400.0)]),
            }),
            group_count: Some(2),
            editors: vec![editor(1, "a.md"), editor(2, "b.md")],
            focused_group: Some(1),
        });
        assert_eq!(
            resolution.columns,
            vec![SurfaceColumn::new(["a.md", "b.md"])]
        );
        assert_eq!(resolution.orientation.as_deref(), Some("v"));
        assert_eq!(resolution.position.as_deref(), Some("bottom"));
        assert_eq!(resolution.column_order, SurfaceColumnOrder::Editor);
    }

    #[test]
    fn another_editor_window_makes_the_order_unknown() {
        let resolution = resolve_group_layout(&GroupLayoutEvidence {
            layout: Some(EditorGroupLayout {
                orientation: Some(0),
                groups: Some(vec![leaf(1.0), leaf(1.0)]),
            }),
            group_count: Some(3),
            editors: vec![editor(1, "a.md"), editor(3, "c.md")],
            focused_group: Some(0),
        });
        assert_eq!(resolution.source, "ordinal");
        assert_eq!(resolution.column_order, SurfaceColumnOrder::Unknown);
        assert_eq!(
            resolution.reason.as_deref(),
            Some("layout_group_count_mismatch")
        );
        assert_eq!(resolution.columns.len(), 3);
    }
}
