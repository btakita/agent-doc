//! The Remote Dev forwarded editor surface behind the hot-reloaded native
//! boundary (GH #326 phase 4, the #295 "path 2" lever).
//!
//! On a JetBrains modular backend, the JetBrains Client measures its main
//! window's split geometry and `client_measured_layout` turns that into
//! columns (GH #320 option B). The backend's `RemoteClientLayoutForwarder`
//! then publishes those columns to the Project Controller as an
//! `editor_surface`. It used to decide in Kotlin (`RemoteClientSurfacePayload.build`)
//! which filesystem root the forward addresses, how the measured paths are
//! rewritten against that root, and what payload is sent. A change to any of
//! those rules needed a plugin generation, and on the exact-263 modular package
//! that is an IDE restart. They live here now: the forwarder reports the
//! measured layout and the focused document's own root
//! ([`RemoteClientSurfaceEvidence`]) and asks the `remote_client_surface`
//! derivation what to publish.
//!
//! The root choice and path normalization are the `sync_layout` rules
//! ([`derive_sync_layout`]), so the forward, the manual sync, and the automatic
//! surface always agree on the root. The payload's order provenance is
//! `editor`, because the columns came from measured geometry, and it owns pane
//! membership and order only (`preserve_focus`): the snapshot names each
//! split's selected document but not which split has keyboard focus, so it must
//! not move tmux focus.
//!
//! [`derive_remote_client_surface`] is pure except for the nearest
//! `.agent-doc` project root of a document, a filesystem fact the IO crate
//! injects. The plugin keeps a frozen Kotlin copy of these rules for a native
//! library that cannot answer; one parity fixture
//! (`editors/jetbrains/src/test/resources/editor-derivations/remote_client_surface.json`)
//! is answered by both. New rules land here, never there.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::sync_layout::{SyncEditorLayout, SyncLayoutColumn, SyncLayoutEvidence};
use crate::{EditorSurface, SurfaceColumn, SurfaceColumnOrder, derive_sync_layout};

/// One measured main-window layout the forwarder publishes. Every field is
/// optional on the wire: after a hot reload this library is newer than the
/// plugin generation calling it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteClientSurfaceEvidence {
    /// The IDE project's base path.
    #[serde(default)]
    pub base_path: Option<String>,
    /// The focused document's own project root, as the plugin resolved it
    /// (`TerminalUtil.resolveProjectPath`): the root a single-root layout keeps.
    #[serde(default)]
    pub fallback_root: Option<String>,
    /// The selected document of the measured layout (absolute).
    #[serde(default)]
    pub focused: Option<String>,
    /// The measured columns in left-to-right order (absolute paths), the
    /// `client_measured_layout` answer.
    #[serde(default)]
    pub columns: Vec<Vec<String>>,
}

/// What the forwarder publishes: the controller root and its `editor_surface`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteClientSurface {
    /// The root that spans the measured layout: the controller the forward
    /// addresses.
    pub project_root: String,
    /// The `editor_surface_observe` payload.
    pub surface: EditorSurface,
}

fn distinct<'a>(items: impl IntoIterator<Item = &'a String>) -> Vec<String> {
    let mut seen = HashSet::new();
    items
        .into_iter()
        .filter(|item| seen.insert(item.as_str()))
        .cloned()
        .collect()
}

/// Derive the forwarded surface from one measured layout.
///
/// `nearest_root` answers the nearest `.agent-doc` project root of a
/// document, or `None` when it has none.
pub fn derive_remote_client_surface(
    evidence: &RemoteClientSurfaceEvidence,
    nearest_root: impl FnMut(&str) -> Option<String>,
) -> RemoteClientSurface {
    let focused = evidence.focused.clone().unwrap_or_default();
    let visible = distinct(evidence.columns.iter().flatten());
    let sync = derive_sync_layout(
        &SyncLayoutEvidence {
            base_path: evidence.base_path.clone(),
            fallback_root: evidence.fallback_root.clone(),
            focused: Some(focused.clone()),
            visible: visible.clone(),
            layout: Some(SyncEditorLayout {
                columns: evidence
                    .columns
                    .iter()
                    .map(|files| SyncLayoutColumn {
                        files: files.clone(),
                    })
                    .collect(),
                column_order: SurfaceColumnOrder::Editor.as_str().to_string(),
            }),
            foreground: Some(false),
            chosen_root: None,
        },
        nearest_root,
    );
    let columns = sync
        .layout
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
    let open =
        crate::surface_projection::prioritize_open_documents(&focused, &visible, &visible, &[]);
    RemoteClientSurface {
        project_root: sync.project_root,
        surface: EditorSurface {
            focused,
            visible,
            open,
            columns,
            column_order: SurfaceColumnOrder::Editor,
            force_reconcile: false,
            focus_only: false,
            preserve_focus: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn a_measured_split_publishes_editor_ordered_columns_that_preserve_focus() {
        let forwarded = derive_remote_client_surface(
            &RemoteClientSurfaceEvidence {
                base_path: Some("/repo".into()),
                fallback_root: Some("/repo".into()),
                focused: Some("/repo/a.md".into()),
                columns: vec![strings(&["/repo/b.md"]), strings(&["/repo/a.md"])],
            },
            |_| Some("/repo".into()),
        );
        assert_eq!(forwarded.project_root, "/repo");
        assert_eq!(
            forwarded.surface.columns,
            [
                SurfaceColumn::new(["/repo/b.md"]),
                SurfaceColumn::new(["/repo/a.md"])
            ]
        );
        assert_eq!(forwarded.surface.column_order, SurfaceColumnOrder::Editor);
        assert!(forwarded.surface.preserve_focus);
        assert!(!forwarded.surface.force_reconcile);
        assert!(!forwarded.surface.focus_only);
        assert_eq!(forwarded.surface.visible, ["/repo/b.md", "/repo/a.md"]);
        assert_eq!(forwarded.surface.open, ["/repo/a.md", "/repo/b.md"]);
    }

    #[test]
    fn a_cross_root_split_addresses_the_workspace_root() {
        let forwarded = derive_remote_client_surface(
            &RemoteClientSurfaceEvidence {
                base_path: Some("/repo".into()),
                fallback_root: Some("/repo/src/one".into()),
                focused: Some("/repo/src/one/a.md".into()),
                columns: vec![
                    strings(&["/repo/src/one/a.md"]),
                    strings(&["/repo/src/two/b.md"]),
                ],
            },
            |file| Some(file.rsplit_once('/').unwrap().0.into()),
        );
        assert_eq!(forwarded.project_root, "/repo");
        assert_eq!(forwarded.surface.columns.len(), 2);
    }

    #[test]
    fn an_empty_observation_still_answers() {
        let forwarded =
            derive_remote_client_surface(&RemoteClientSurfaceEvidence::default(), |_| None);
        assert_eq!(forwarded.project_root, "");
        assert!(forwarded.surface.columns.is_empty());
        assert_eq!(forwarded.surface.focused, "");
        assert!(forwarded.surface.preserve_focus);
    }
}
