//! Sync Tmux Layout `--col` snapshot decisions behind the hot-reloaded native
//! boundary (GH #326 phase 3, the #295 "path 2" lever).
//!
//! The manual `Sync Tmux Layout` action used to decide in Kotlin, on the EDT,
//! which filesystem root the sync addresses (`chooseSyncProjectRoot`), how the
//! detected layout's paths are rewritten against that root
//! (`normalizeEditorLayout`, `absolutizeEditorLayout`), which `--col` columns
//! it publishes (`undetectedLayoutColumns`, GH #81), and whether the
//! exact-visible snapshot may be published at all (`exactVisibleSyncDecision`,
//! GH #154). A change to any of those rules needed a plugin generation, and on
//! the exact-263 modular package that is an IDE restart. They live here now:
//! the plugin captures what the editor *is* ([`SyncLayoutEvidence`]) and its
//! background thread asks the `sync_layout` derivation what it *means*.
//!
//! The automatic surface applies the same root choice and path normalization
//! to the payload `surface_projection` derives, so it asks this derivation too.
//!
//! Phase 3b moved the last two callers of these rules here. Run Agent Doc's
//! `--col` fallback (`TerminalUtil.editorSnapshotRouteLayout`) addresses the
//! route's own root, which it chose before it read the split, so it passes that
//! root as [`SyncLayoutEvidence::chosen_root`] and reads the columns and the
//! route's layout mode ([`SyncLayoutSnapshot::layout_mode`], GH #111). The
//! `TmuxPaneFocusSync` poll asks which controllers may own the tmux focus and
//! reads [`SyncLayoutSnapshot::candidate_roots`].
//!
//! [`derive_sync_layout`] is pure except for the nearest `.agent-doc` project
//! root of a visible document, a filesystem fact the IO crate injects. The
//! plugin keeps a frozen Kotlin copy of these rules for a native library that
//! cannot answer; one parity fixture
//! (`editors/jetbrains/src/test/resources/editor-derivations/sync_layout.json`)
//! is answered by both. New rules land here, never there.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::SurfaceColumn;
use crate::surface_projection::is_absolute_path;

/// `EditorLayout.columnOrder`'s value for a layout that does not name one.
fn editor_column_order() -> String {
    "editor".to_string()
}

/// A split layout: columns in left-to-right order and their order provenance
/// (GH #320). The provenance stays a string so a label a newer layout
/// derivation adds passes through unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncEditorLayout {
    #[serde(default)]
    pub columns: Vec<SyncLayoutColumn>,
    #[serde(default = "editor_column_order")]
    pub column_order: String,
}

/// One column of a [`SyncEditorLayout`]. A column may be empty: a split that
/// shows no session document keeps its pane position.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncLayoutColumn {
    #[serde(default)]
    pub files: Vec<String>,
}

impl From<SurfaceColumn> for SyncLayoutColumn {
    fn from(column: SurfaceColumn) -> Self {
        Self {
            files: column.files,
        }
    }
}

/// What one Sync Tmux Layout (or automatic surface delivery) observed. Every
/// field is optional on the wire: after a hot reload this library is newer
/// than the plugin generation calling it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncLayoutEvidence {
    /// The IDE project's base path.
    #[serde(default)]
    pub base_path: Option<String>,
    /// The focused document's own project root, as the plugin resolved it
    /// (`TerminalUtil.resolveProject`): the root a single-root layout keeps.
    #[serde(default)]
    pub fallback_root: Option<String>,
    /// The focused session document (absolute).
    #[serde(default)]
    pub focused: Option<String>,
    /// The visible session documents (absolute), focused first.
    #[serde(default)]
    pub visible: Vec<String>,
    /// The detected split layout, project relative or absolute; absent when
    /// the split could not be read.
    #[serde(default)]
    pub layout: Option<SyncEditorLayout>,
    /// Whether a foreground (user) action asks: only a foreground sync refuses
    /// a layout that omits the focused document. Absent means foreground.
    #[serde(default)]
    pub foreground: Option<bool>,
    /// The root the caller already chose (GH #326 phase 3b): Run Agent Doc
    /// routes to the focused document's own root before it reads the split.
    /// When present and not blank, the snapshot addresses this root instead of
    /// choosing the one that spans the visible layout, and normalizes the
    /// layout against it. Absent (an older plugin generation, or a caller that
    /// lets the derivation choose) keeps the phase 3 choice.
    #[serde(default)]
    pub chosen_root: Option<String>,
}

/// Whether the exact-visible `--col` snapshot may be published (GH #154).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncLayoutDecision {
    /// Publish [`SyncLayoutSnapshot::columns`].
    Publish,
    /// The editor split could not be read; exact-visible is structural
    /// authority, so the focused-file fallback must not replace it.
    #[default]
    RefuseUnknownLayout,
    /// The focused document is in no column: the split was read while focus
    /// moved, so the projection is stale.
    RefuseStaleFocusProjection,
}

/// The derived sync snapshot.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncLayoutSnapshot {
    /// The root that spans the visible layout: the controller the sync
    /// addresses and the root its columns are normalized against.
    pub project_root: String,
    /// The detected layout normalized against [`Self::project_root`] and made
    /// absolute; absent when there was none or every column was empty.
    #[serde(default)]
    pub layout: Option<SyncEditorLayout>,
    /// The `--col` arguments: one per detected column of a multi-column
    /// layout, otherwise one per visible document (GH #81), never none.
    pub columns: Vec<String>,
    /// The order label the columns carry (GH #112/#185/#320): the layout's own
    /// for a multi-column layout, otherwise `unknown`.
    pub column_order: String,
    pub decision: SyncLayoutDecision,
    /// What the columns assert to the controller (GH #111): `exact` when they
    /// are a detected multi-column layout, a positive observation of the whole
    /// visible layout; otherwise `ensure`, which only asks that each named
    /// document has a column and keeps the retained layout around it. A string,
    /// so a mode a newer library adds passes through.
    #[serde(default = "ensure_layout_mode")]
    pub layout_mode: String,
    /// The controller roots that may own this observation's tmux focus, most
    /// specific first and without repeats (the `TmuxPaneFocusSync` probe): the
    /// focused document's own root, then [`Self::project_root`] when there are
    /// visible documents, then each visible document's own root (its nearest
    /// `.agent-doc` root, else the base path, else its directory), then the
    /// base path. The spanning root is listed only when a fallback root was
    /// reported, as the plugin's probe did.
    #[serde(default)]
    pub candidate_roots: Vec<String>,
}

/// [`SyncLayoutSnapshot::layout_mode`] for columns that only name documents.
pub const LAYOUT_MODE_ENSURE: &str = "ensure";
/// [`SyncLayoutSnapshot::layout_mode`] for a detected multi-column layout.
pub const LAYOUT_MODE_EXACT: &str = "exact";

fn ensure_layout_mode() -> String {
    LAYOUT_MODE_ENSURE.to_string()
}

fn distinct<'a>(items: impl IntoIterator<Item = &'a String>) -> Vec<String> {
    let mut seen = HashSet::new();
    items
        .into_iter()
        .filter(|item| seen.insert(item.as_str()))
        .cloned()
        .collect()
}

/// `java.io.File(parent, child).path` for a relative `child`.
fn join(parent: &str, child: &str) -> String {
    format!("{}/{child}", parent.trim_end_matches('/'))
}

/// `SyncLayoutAction.chooseSyncProjectRoot`: the root of the visible documents
/// when they share one; the workspace base path when they span several roots,
/// or when none has a root and one lies outside the fallback root; otherwise
/// the focused document's own root.
pub fn choose_sync_project_root(
    base_path: Option<&str>,
    fallback_root: &str,
    visible: &[String],
    mut nearest_root: impl FnMut(&str) -> Option<String>,
) -> String {
    let roots: Vec<String> = visible
        .iter()
        .filter_map(|file| nearest_root(file))
        .collect();
    let roots = distinct(roots.iter());
    if roots.len() <= 1 {
        if roots.is_empty()
            && let Some(base) = base_path
            && visible.iter().any(|file| {
                file != fallback_root && !file.starts_with(&format!("{fallback_root}/"))
            })
        {
            return base.to_string();
        }
        return roots
            .into_iter()
            .next()
            .unwrap_or_else(|| fallback_root.to_string());
    }
    base_path.unwrap_or(fallback_root).to_string()
}

/// A layout whose columns are all empty is no layout.
fn non_empty(layout: SyncEditorLayout) -> Option<SyncEditorLayout> {
    layout
        .columns
        .iter()
        .any(|column| !column.files.is_empty())
        .then_some(layout)
}

/// `SyncLayoutAction.normalizeEditorLayout`: a layout detected relative to the
/// workspace base path, rewritten relative to a nested `project_root`. A file
/// under that root loses the root's prefix; any other relative file becomes
/// absolute under the base path. Blank files are dropped.
pub fn normalize_layout(
    base_path: Option<&str>,
    project_root: &str,
    layout: SyncEditorLayout,
) -> Option<SyncEditorLayout> {
    let Some(base) = base_path.filter(|base| *base != project_root) else {
        return Some(layout);
    };
    let root_prefix = project_root
        .strip_prefix(&format!("{base}/"))
        .unwrap_or(project_root)
        .trim_matches('/');
    if root_prefix.is_empty() {
        return Some(layout);
    }
    let nested = format!("{root_prefix}/");
    let columns = layout
        .columns
        .into_iter()
        .map(|column| SyncLayoutColumn {
            files: column
                .files
                .into_iter()
                .filter_map(|file| {
                    if file.trim().is_empty() {
                        None
                    } else if is_absolute_path(&file) {
                        Some(file)
                    } else if let Some(inside) = file.strip_prefix(&nested) {
                        Some(inside.to_string())
                    } else {
                        Some(join(base, &file))
                    }
                })
                .collect(),
        })
        .collect();
    non_empty(SyncEditorLayout {
        columns,
        column_order: layout.column_order,
    })
}

/// `SyncLayoutAction.absolutizeEditorLayout`: resolve relative files against
/// `project_root`, dropping blanks.
pub fn absolutize_layout(project_root: &str, layout: SyncEditorLayout) -> Option<SyncEditorLayout> {
    let columns = layout
        .columns
        .into_iter()
        .map(|column| SyncLayoutColumn {
            files: column
                .files
                .into_iter()
                .filter_map(|file| {
                    if file.trim().is_empty() {
                        None
                    } else if is_absolute_path(&file) {
                        Some(file)
                    } else {
                        Some(join(project_root, &file))
                    }
                })
                .collect(),
        })
        .collect();
    non_empty(SyncEditorLayout {
        columns,
        column_order: layout.column_order,
    })
}

/// `SyncLayoutAction.undetectedLayoutColumns` (GH #81): every visible document
/// is the selected tab of its own editor window, so each is its own column,
/// in the order observed.
pub fn undetected_layout_columns(visible: &[String]) -> Vec<String> {
    distinct(visible.iter().filter(|file| !file.trim().is_empty()))
}

/// `SyncLayoutAction.buildSyncColumns`.
fn sync_columns(visible: &[String], layout: Option<&SyncEditorLayout>) -> Vec<String> {
    match layout {
        Some(layout) if layout.columns.len() > 1 => layout
            .columns
            .iter()
            .map(|column| column.files.join(","))
            .collect(),
        _ => {
            let columns = undetected_layout_columns(visible);
            if columns.is_empty() {
                vec![String::new()]
            } else {
                columns
            }
        }
    }
}

/// `TmuxPaneFocusSync.candidateProjectRoots`: the roots a tmux focus probe
/// asks, most specific first.
fn candidate_roots(
    evidence: &SyncLayoutEvidence,
    project_root: &str,
    nearest_root: &mut impl FnMut(&str) -> Option<String>,
) -> Vec<String> {
    let base_path = evidence.base_path.as_deref();
    let fallback_root = evidence
        .fallback_root
        .as_deref()
        .filter(|root| !root.trim().is_empty());
    let mut roots: Vec<String> = Vec::new();
    roots.extend(fallback_root.map(str::to_string));
    if fallback_root.is_some() && !evidence.visible.is_empty() {
        roots.push(project_root.to_string());
    }
    for file in &evidence.visible {
        let own = nearest_root(file)
            .or_else(|| base_path.map(str::to_string))
            .unwrap_or_else(|| {
                std::path::Path::new(file)
                    .parent()
                    .map(|parent| parent.to_string_lossy().into_owned())
                    .filter(|parent| !parent.is_empty())
                    .unwrap_or_else(|| "/".to_string())
            });
        roots.push(own);
    }
    roots.extend(base_path.map(str::to_string));
    distinct(roots.iter().filter(|root| !root.trim().is_empty()))
}

/// Derive the Sync Tmux Layout snapshot from one observation.
///
/// `nearest_root` answers the nearest `.agent-doc` project root of a visible
/// document, or `None` when it has none. It is asked at most once per document.
pub fn derive_sync_layout(
    evidence: &SyncLayoutEvidence,
    mut probe_root: impl FnMut(&str) -> Option<String>,
) -> SyncLayoutSnapshot {
    let mut known: std::collections::HashMap<String, Option<String>> =
        std::collections::HashMap::new();
    let mut nearest_root = |file: &str| -> Option<String> {
        known
            .entry(file.to_string())
            .or_insert_with(|| probe_root(file))
            .clone()
    };
    let base_path = evidence.base_path.as_deref();
    let fallback_root = evidence
        .fallback_root
        .as_deref()
        .filter(|root| !root.trim().is_empty())
        .or(base_path)
        .unwrap_or_default();
    let project_root = match evidence
        .chosen_root
        .as_deref()
        .filter(|root| !root.trim().is_empty())
    {
        Some(chosen) => chosen.to_string(),
        None => choose_sync_project_root(
            base_path,
            fallback_root,
            &evidence.visible,
            &mut nearest_root,
        ),
    };
    let layout = evidence
        .layout
        .clone()
        .and_then(|layout| normalize_layout(base_path, &project_root, layout))
        .and_then(|layout| absolutize_layout(&project_root, layout));
    let columns = sync_columns(&evidence.visible, layout.as_ref());
    let multi_column = matches!(&layout, Some(layout) if layout.columns.len() > 1);
    let column_order = match &layout {
        Some(layout) if multi_column => layout.column_order.clone(),
        _ => "unknown".to_string(),
    };
    let layout_mode = if multi_column {
        LAYOUT_MODE_EXACT
    } else {
        LAYOUT_MODE_ENSURE
    }
    .to_string();
    let focused = evidence
        .focused
        .as_deref()
        .filter(|focused| !focused.is_empty());
    let decision = match (&layout, focused) {
        (None, _) => SyncLayoutDecision::RefuseUnknownLayout,
        (Some(_), Some(focused))
            if evidence.foreground.unwrap_or(true)
                && !columns
                    .iter()
                    .any(|column| column.split(',').any(|file| file.trim() == focused)) =>
        {
            SyncLayoutDecision::RefuseStaleFocusProjection
        }
        (Some(_), _) => SyncLayoutDecision::Publish,
    };
    let candidate_roots = candidate_roots(evidence, &project_root, &mut nearest_root);
    SyncLayoutSnapshot {
        project_root,
        layout,
        columns,
        column_order,
        decision,
        layout_mode,
        candidate_roots,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(columns: &[&[&str]], order: &str) -> SyncEditorLayout {
        SyncEditorLayout {
            columns: columns
                .iter()
                .map(|files| SyncLayoutColumn {
                    files: files.iter().map(|file| file.to_string()).collect(),
                })
                .collect(),
            column_order: order.to_string(),
        }
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    fn no_root(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn a_single_root_layout_stays_scoped_to_that_root() {
        let visible = strings(&["/repo/src/app/tasks/a.md", "/repo/src/app/tasks/b.md"]);
        let root = choose_sync_project_root(Some("/repo"), "/repo/src/app", &visible, |_| {
            Some("/repo/src/app".into())
        });
        assert_eq!(root, "/repo/src/app");
    }

    #[test]
    fn a_cross_root_layout_uses_the_workspace_root() {
        let visible = strings(&["/repo/src/one/a.md", "/repo/src/two/b.md"]);
        let root = choose_sync_project_root(Some("/repo"), "/repo/src/one", &visible, |file| {
            Some(file.rsplit_once('/').unwrap().0.into())
        });
        assert_eq!(root, "/repo");
        let headless = choose_sync_project_root(None, "/repo/src/one", &visible, |file| {
            Some(file.rsplit_once('/').unwrap().0.into())
        });
        assert_eq!(headless, "/repo/src/one");
    }

    #[test]
    fn rootless_documents_outside_the_fallback_root_use_the_base_path() {
        let visible = strings(&["/repo/src/app/a.md", "/elsewhere/b.md"]);
        assert_eq!(
            choose_sync_project_root(Some("/repo"), "/repo/src/app", &visible, no_root),
            "/repo"
        );
        let inside = strings(&["/repo/src/app/a.md"]);
        assert_eq!(
            choose_sync_project_root(Some("/repo"), "/repo/src/app", &inside, no_root),
            "/repo/src/app"
        );
    }

    #[test]
    fn normalization_rewrites_workspace_relative_files_into_the_nested_root() {
        let normalized = normalize_layout(
            Some("/repo"),
            "/repo/src/app",
            layout(
                &[
                    &["src/app/tasks/one.md"],
                    &["src/app/tasks/two.md", "tasks/x.md"],
                ],
                "retained",
            ),
        );
        assert_eq!(
            normalized,
            Some(layout(
                &[&["tasks/one.md"], &["tasks/two.md", "/repo/tasks/x.md"]],
                "retained"
            ))
        );
        let empty = normalize_layout(Some("/repo"), "/repo/src/app", layout(&[&[""]], "editor"));
        assert_eq!(empty, None);
        let same_root = layout(&[&[""]], "editor");
        assert_eq!(
            normalize_layout(Some("/repo"), "/repo", same_root.clone()),
            Some(same_root)
        );
    }

    #[test]
    fn absolutizing_keeps_empty_columns_and_absolute_files() {
        let absolute = absolutize_layout(
            "/repo/src/app",
            layout(&[&[], &["tasks/a.md", "/already/b.md", " "]], "editor"),
        );
        assert_eq!(
            absolute,
            Some(layout(
                &[&[], &["/repo/src/app/tasks/a.md", "/already/b.md"]],
                "editor"
            ))
        );
        assert_eq!(absolutize_layout("/repo", layout(&[&[]], "editor")), None);
    }

    #[test]
    fn an_unreadable_split_refuses_instead_of_collapsing_to_the_focused_file() {
        let evidence = SyncLayoutEvidence {
            base_path: Some("/repo".into()),
            fallback_root: Some("/repo".into()),
            focused: Some("/repo/a.md".into()),
            visible: strings(&["/repo/a.md"]),
            ..Default::default()
        };
        let snapshot = derive_sync_layout(&evidence, no_root);
        assert_eq!(snapshot.decision, SyncLayoutDecision::RefuseUnknownLayout);
        assert_eq!(snapshot.columns, ["/repo/a.md"]);
        assert_eq!(snapshot.column_order, "unknown");
    }

    #[test]
    fn a_split_without_the_focused_document_refuses_only_a_foreground_sync() {
        let mut evidence = SyncLayoutEvidence {
            base_path: Some("/repo".into()),
            fallback_root: Some("/repo".into()),
            focused: Some("/repo/c.md".into()),
            visible: strings(&["/repo/c.md", "/repo/a.md", "/repo/b.md"]),
            layout: Some(layout(&[&["a.md"], &["b.md"]], "editor")),
            ..Default::default()
        };
        let snapshot = derive_sync_layout(&evidence, no_root);
        assert_eq!(
            snapshot.decision,
            SyncLayoutDecision::RefuseStaleFocusProjection
        );
        evidence.foreground = Some(false);
        let background = derive_sync_layout(&evidence, no_root);
        assert_eq!(background.decision, SyncLayoutDecision::Publish);
        assert_eq!(background.columns, ["/repo/a.md", "/repo/b.md"]);
        assert_eq!(background.column_order, "editor");
    }

    #[test]
    fn an_empty_observation_still_answers() {
        let snapshot = derive_sync_layout(&SyncLayoutEvidence::default(), no_root);
        assert_eq!(snapshot.project_root, "");
        assert_eq!(snapshot.columns, [""]);
        assert_eq!(snapshot.column_order, "unknown");
        assert_eq!(snapshot.decision, SyncLayoutDecision::RefuseUnknownLayout);
    }

    #[test]
    fn a_chosen_root_is_addressed_instead_of_the_spanning_root() {
        // GH #326 phase 3b: Run Agent Doc routes to the focused document's own
        // root, so a cross-root split is normalized against that root and its
        // workspace documents stay absolute.
        let evidence = SyncLayoutEvidence {
            base_path: Some("/repo".into()),
            fallback_root: Some("/repo/src/one".into()),
            chosen_root: Some("/repo/src/one".into()),
            focused: Some("/repo/src/one/a.md".into()),
            visible: strings(&["/repo/src/one/a.md", "/repo/src/two/b.md"]),
            layout: Some(layout(&[&["src/one/a.md"], &["src/two/b.md"]], "retained")),
            foreground: Some(false),
        };
        let roots = |file: &str| Some(file.rsplit_once('/').unwrap().0.to_string());
        let snapshot = derive_sync_layout(&evidence, roots);
        assert_eq!(snapshot.project_root, "/repo/src/one");
        assert_eq!(
            snapshot.columns,
            ["/repo/src/one/a.md", "/repo/src/two/b.md"]
        );
        assert_eq!(snapshot.column_order, "retained");
        assert_eq!(snapshot.layout_mode, LAYOUT_MODE_EXACT);
        let chosen_by_rule = derive_sync_layout(
            &SyncLayoutEvidence {
                chosen_root: Some(" ".into()),
                ..evidence
            },
            roots,
        );
        assert_eq!(chosen_by_rule.project_root, "/repo");
    }

    #[test]
    fn only_a_detected_multi_column_layout_asserts_the_exact_layout() {
        let mut evidence = SyncLayoutEvidence {
            base_path: Some("/repo".into()),
            fallback_root: Some("/repo".into()),
            focused: Some("/repo/a.md".into()),
            visible: strings(&["/repo/a.md", "/repo/b.md"]),
            layout: Some(layout(&[&["a.md"]], "editor")),
            ..Default::default()
        };
        let single = derive_sync_layout(&evidence, no_root);
        assert_eq!(single.layout_mode, LAYOUT_MODE_ENSURE);
        assert_eq!(single.columns, ["/repo/a.md", "/repo/b.md"]);
        evidence.layout = None;
        assert_eq!(
            derive_sync_layout(&evidence, no_root).layout_mode,
            LAYOUT_MODE_ENSURE
        );
    }

    #[test]
    fn candidate_roots_run_from_the_focused_root_to_the_base_path() {
        let evidence = SyncLayoutEvidence {
            base_path: Some("/repo".into()),
            fallback_root: Some("/repo/src/one".into()),
            focused: Some("/repo/src/one/a.md".into()),
            visible: strings(&["/repo/src/one/a.md", "/repo/src/two/b.md", "/repo/c.md"]),
            ..Default::default()
        };
        let snapshot = derive_sync_layout(&evidence, |file| {
            (!file.starts_with("/repo/c")).then(|| file.rsplit_once('/').unwrap().0.to_string())
        });
        assert_eq!(
            snapshot.candidate_roots,
            ["/repo/src/one", "/repo", "/repo/src/two"]
        );
        // Without a focused root the spanning root is not a candidate; without
        // a base path a rootless document's own directory is.
        let unfocused = derive_sync_layout(
            &SyncLayoutEvidence {
                visible: strings(&["/elsewhere/notes/x.md"]),
                ..Default::default()
            },
            no_root,
        );
        assert_eq!(unfocused.candidate_roots, ["/elsewhere/notes"]);
        assert!(
            derive_sync_layout(&SyncLayoutEvidence::default(), no_root)
                .candidate_roots
                .is_empty()
        );
    }

    #[test]
    fn each_document_is_probed_for_its_root_once() {
        let mut probes = Vec::new();
        derive_sync_layout(
            &SyncLayoutEvidence {
                base_path: Some("/repo".into()),
                fallback_root: Some("/repo".into()),
                visible: strings(&["/repo/a.md", "/repo/b.md", "/repo/a.md"]),
                ..Default::default()
            },
            |file: &str| {
                probes.push(file.to_string());
                None
            },
        );
        assert_eq!(probes, ["/repo/a.md", "/repo/b.md"]);
    }

    #[test]
    fn an_answer_without_the_phase_3b_fields_still_parses() {
        let parsed: SyncLayoutSnapshot = serde_json::from_str(
            r#"{"project_root":"/r","columns":["/r/a.md"],"column_order":"unknown","decision":"publish"}"#,
        )
        .unwrap();
        assert_eq!(parsed.layout_mode, LAYOUT_MODE_ENSURE);
        assert!(parsed.candidate_roots.is_empty());
        let evidence: SyncLayoutEvidence =
            serde_json::from_str(r#"{"visible":["/r/a.md"]}"#).unwrap();
        assert_eq!(evidence.chosen_root, None);
    }

    #[test]
    fn a_layout_without_an_order_label_is_editor_geometry() {
        let parsed: SyncLayoutEvidence =
            serde_json::from_str(r#"{"layout":{"columns":[{"files":["a.md"]},{}]}}"#).unwrap();
        assert_eq!(parsed.layout, Some(layout(&[&["a.md"], &[]], "editor")));
    }
}
