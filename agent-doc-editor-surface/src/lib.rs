//! Editor-surface observation and the tmux intent derived from it
//! (`#jbpluginlazilyeffects`).
//!
//! An editor reports *what it looks like*: which markdown documents are visible,
//! how they are arranged in columns, and which one has focus. What tmux should
//! do about that is a **derivation** over successive observations — focus moved
//! but the layout did not, so select a pane; the layout changed, so reconcile it;
//! nothing changed, so do nothing.
//!
//! That derivation used to live in the plugins. `AutomaticCommandPlanner` in the
//! JetBrains plugin decided it in Kotlin, held the previous observation in three
//! `@Volatile` fields, and picked a command to submit; the VS Code extension
//! carried its own copy. Two consequences followed. The rule was written twice
//! and could drift, and — because the decision lived beside a mutable field
//! rather than beside the data — every editor event had to remember to consult
//! it. A plugin that forgets to call the planner silently stops syncing.
//!
//! This crate is the single decision, as pure total functions. It performs no
//! IO, spawns nothing, and holds no state of its own: [`SurfaceTracking`] is a
//! plain value that [`SurfaceTracking::advance`] folds one observation into,
//! returning the next tracking value and the intent that observation implies.
//!
//! Keeping the history fold in plain data — rather than spreading it across
//! cells — is deliberate. The equivalent mistake in `idle_revision.rs` wrote
//! three source cells by hand from one method and shipped two ordering bugs;
//! neither is expressible once the history is one value advanced by one
//! function. The reactive plane above this crate holds a `SurfaceTracking` and
//! derives [`SurfaceIntent`] from it, so the intent updates because an
//! observation arrived rather than because a caller remembered to ask.

pub mod graph;

pub use graph::{EditorSurfaceState, SurfaceFold};

use serde::{Deserialize, Serialize};

/// Separator between columns in a visible signature. A control character so it
/// cannot occur in a path.
const COLUMN_SEPARATOR: char = '\u{0}';
/// Separator between files within one column.
const FILE_SEPARATOR: char = '\u{1}';

/// One column of the editor's split layout, in visual order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceColumn {
    /// Absolute paths of the documents in this column.
    pub files: Vec<String>,
}

impl SurfaceColumn {
    pub fn new(files: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            files: files.into_iter().map(Into::into).collect(),
        }
    }
}

/// What the editor looks like right now, as reported by a plugin.
///
/// This is an **observation**: every field is something the editor saw, never
/// something anyone derived. It is the value the reactive plane stores in a
/// `Source`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorSurface {
    /// The document the operator is focused on. Empty when the editor has no
    /// markdown selection, which makes the surface inert.
    #[serde(default)]
    pub focused: String,
    /// Every visible markdown document, in no particular order.
    #[serde(default)]
    pub visible: Vec<String>,
    /// The split layout. Empty means "layout not detected"; the signature then
    /// falls back to the sorted visible set.
    #[serde(default)]
    pub columns: Vec<SurfaceColumn>,
    /// Whether tmux currently matches this layout, when the caller knows.
    ///
    /// `Some(false)` is the case a pure focus decision gets wrong: the editor's
    /// split model is unchanged but the tmux panes have drifted or swapped, so
    /// selecting a pane would pick the right document in the wrong column. Kept
    /// three-valued on purpose — "I did not look" (`None`) must not read as "it
    /// has drifted", the inversion that made the supervisor's idle watch issue
    /// its expensive probes 120x too often.
    #[serde(default)]
    pub layout_synced: Option<bool>,
    /// The operator asked for a reconcile explicitly, so skip the unchanged-
    /// observation shortcut.
    #[serde(default)]
    pub force_reconcile: bool,
}

impl EditorSurface {
    /// A stable identity for "which documents are visible, and how are they
    /// arranged". Two observations with the same signature describe the same
    /// tmux layout, so only focus can differ between them.
    ///
    /// Derived from the columns when the layout was detected, because column
    /// membership is what tmux mirrors; otherwise from the sorted, de-duplicated
    /// visible set, which is all the caller knows.
    pub fn visible_signature(&self) -> String {
        if !self.columns.is_empty() {
            return self
                .columns
                .iter()
                .map(|column| {
                    let mut seen = Vec::new();
                    for file in &column.files {
                        if !file.is_empty() && !seen.contains(file) {
                            seen.push(file.clone());
                        }
                    }
                    seen.join(&FILE_SEPARATOR.to_string())
                })
                .collect::<Vec<_>>()
                .join(&COLUMN_SEPARATOR.to_string());
        }
        let mut files: Vec<&String> = Vec::new();
        for file in &self.visible {
            if !files.contains(&file) {
                files.push(file);
            }
        }
        files.sort();
        files
            .into_iter()
            .cloned()
            .collect::<Vec<_>>()
            .join(&COLUMN_SEPARATOR.to_string())
    }

    /// An observation with nothing visible cannot imply any tmux consequence.
    pub fn is_inert(&self) -> bool {
        self.visible.is_empty() || self.focused.is_empty()
    }
}

/// What tmux should do about the current editor surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SurfaceIntent {
    /// Nothing to do: the surface is inert, or it is the same surface tmux was
    /// already reconciled against.
    Idle,
    /// Select the pane showing `document`. The visible layout is unchanged, so
    /// this is a pure focus move — the one case that should move the operator's
    /// active pane, since layout reconciliation deliberately never does.
    Focus { document: String },
    /// Reconcile the whole layout, then focus `document` inside it.
    Sync {
        columns: Vec<SurfaceColumn>,
        document: String,
    },
}

impl SurfaceIntent {
    pub fn is_idle(&self) -> bool {
        matches!(self, SurfaceIntent::Idle)
    }

    /// The document this intent acts on, if any.
    pub fn document(&self) -> Option<&str> {
        match self {
            SurfaceIntent::Idle => None,
            SurfaceIntent::Focus { document } | SurfaceIntent::Sync { document, .. } => {
                Some(document)
            }
        }
    }
}

/// The part of the decision that depends on history: what tmux was last
/// reconciled against.
///
/// A plain value, folded by [`Self::advance`]. It is not three cells and not a
/// set of mutable fields, because the ordering between "compare against the
/// previous observation" and "record this one as the previous observation" is
/// the whole content of the rule — and that ordering is only unambiguous when
/// one function owns both halves.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceTracking {
    /// The signature tmux was last reconciled against.
    #[serde(default)]
    pub reconciled_signature: Option<String>,
    /// The document tmux was last focused on.
    #[serde(default)]
    pub focused_document: Option<String>,
}

impl SurfaceTracking {
    /// Fold `surface` into this tracking value, returning the next tracking
    /// value and the intent the observation implies.
    ///
    /// Total: every observation produces an answer, and an observation that
    /// implies nothing produces [`SurfaceIntent::Idle`] with the tracking value
    /// unchanged. Advancing on an idle observation is what would make a
    /// duplicate event look like a change on the next one.
    pub fn advance(&self, surface: &EditorSurface) -> (Self, SurfaceIntent) {
        if surface.is_inert() {
            return (self.clone(), SurfaceIntent::Idle);
        }

        let signature = surface.visible_signature();
        let same_layout = self.reconciled_signature.as_deref() == Some(signature.as_str());
        let same_focus = self.focused_document.as_deref() == Some(surface.focused.as_str());

        // Nothing observable changed and nobody claims tmux has drifted.
        if !surface.force_reconcile
            && surface.layout_synced != Some(false)
            && same_layout
            && same_focus
        {
            return (self.clone(), SurfaceIntent::Idle);
        }

        let advanced = Self {
            reconciled_signature: Some(signature),
            focused_document: Some(surface.focused.clone()),
        };

        // tmux has drifted from a layout the editor never changed: focusing
        // would select the right document in the wrong column, so reconcile.
        if surface.layout_synced == Some(false) {
            return (
                advanced,
                SurfaceIntent::Sync {
                    columns: surface.columns.clone(),
                    document: surface.focused.clone(),
                },
            );
        }

        // Same visible layout, different focus: a document-to-document switch.
        // Layout reconciliation neutralizes any internal selection and never
        // moves the active pane, so emitting a sync here would leave the switch
        // dead.
        if same_layout {
            return (
                advanced,
                SurfaceIntent::Focus {
                    document: surface.focused.clone(),
                },
            );
        }

        (
            advanced,
            SurfaceIntent::Sync {
                columns: surface.columns.clone(),
                document: surface.focused.clone(),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn surface(focused: &str, columns: &[&[&str]]) -> EditorSurface {
        let columns: Vec<SurfaceColumn> = columns
            .iter()
            .map(|files| SurfaceColumn::new(files.iter().copied()))
            .collect();
        let visible = columns
            .iter()
            .flat_map(|column| column.files.iter().cloned())
            .collect();
        EditorSurface {
            focused: focused.to_string(),
            visible,
            columns,
            layout_synced: Some(true),
            force_reconcile: false,
        }
    }

    #[test]
    fn the_first_observation_reconciles_the_layout() {
        let (tracking, intent) =
            SurfaceTracking::default().advance(&surface("/a.md", &[&["/a.md"], &["/b.md"]]));
        assert!(matches!(intent, SurfaceIntent::Sync { .. }));
        assert_eq!(intent.document(), Some("/a.md"));
        assert_eq!(tracking.focused_document.as_deref(), Some("/a.md"));
    }

    #[test]
    fn repeating_the_same_observation_implies_nothing() {
        let observed = surface("/a.md", &[&["/a.md"], &["/b.md"]]);
        let (tracking, _) = SurfaceTracking::default().advance(&observed);
        let (again, intent) = tracking.advance(&observed);
        assert_eq!(intent, SurfaceIntent::Idle);
        assert_eq!(
            again, tracking,
            "an idle observation must not advance the tracking value"
        );
    }

    #[test]
    fn moving_focus_within_an_unchanged_layout_is_a_focus_move() {
        let (tracking, _) =
            SurfaceTracking::default().advance(&surface("/a.md", &[&["/a.md"], &["/b.md"]]));
        let (_, intent) = tracking.advance(&surface("/b.md", &[&["/a.md"], &["/b.md"]]));
        assert_eq!(
            intent,
            SurfaceIntent::Focus {
                document: "/b.md".to_string()
            },
            "layout reconciliation never moves the active pane, so a doc switch must focus"
        );
    }

    #[test]
    fn a_changed_layout_reconciles_even_with_unchanged_focus() {
        let (tracking, _) =
            SurfaceTracking::default().advance(&surface("/a.md", &[&["/a.md"], &["/b.md"]]));
        let (_, intent) = tracking.advance(&surface("/a.md", &[&["/a.md"]]));
        assert!(matches!(intent, SurfaceIntent::Sync { .. }));
    }

    #[test]
    fn proven_tmux_drift_reconciles_an_otherwise_unchanged_surface() {
        let observed = surface("/a.md", &[&["/a.md"], &["/b.md"]]);
        let (tracking, _) = SurfaceTracking::default().advance(&observed);
        let drifted = EditorSurface {
            layout_synced: Some(false),
            ..observed.clone()
        };
        let (_, intent) = tracking.advance(&drifted);
        assert!(
            matches!(intent, SurfaceIntent::Sync { .. }),
            "focusing would select the right document in the wrong column"
        );
    }

    #[test]
    fn an_unknown_layout_state_is_not_read_as_drift() {
        let observed = surface("/a.md", &[&["/a.md"], &["/b.md"]]);
        let (tracking, _) = SurfaceTracking::default().advance(&observed);
        let unknown = EditorSurface {
            layout_synced: None,
            ..observed
        };
        let (_, intent) = tracking.advance(&unknown);
        assert_eq!(
            intent,
            SurfaceIntent::Idle,
            "\"I did not look\" must stay distinct from \"I looked and it has drifted\""
        );
    }

    #[test]
    fn force_reconcile_re_emits_but_does_not_change_the_kind() {
        let observed = surface("/a.md", &[&["/a.md"], &["/b.md"]]);
        let (tracking, _) = SurfaceTracking::default().advance(&observed);
        let forced = EditorSurface {
            force_reconcile: true,
            ..observed
        };
        let (_, intent) = tracking.advance(&forced);
        // `force_reconcile` suppresses the unchanged-observation shortcut; it
        // does not claim the layout drifted. Only `layout_synced: Some(false)`
        // is evidence of that, and inventing a sync here would neutralize the
        // active pane the operator just moved.
        assert_eq!(
            intent,
            SurfaceIntent::Focus {
                document: "/a.md".to_string()
            },
        );

        let forced_new_layout = EditorSurface {
            force_reconcile: true,
            ..surface("/a.md", &[&["/a.md"]])
        };
        let (_, intent) = tracking.advance(&forced_new_layout);
        assert!(matches!(intent, SurfaceIntent::Sync { .. }));
    }

    #[test]
    fn an_inert_surface_implies_nothing_and_advances_nothing() {
        let tracking = SurfaceTracking {
            reconciled_signature: Some("sig".to_string()),
            focused_document: Some("/a.md".to_string()),
        };
        for inert in [
            EditorSurface::default(),
            EditorSurface {
                focused: "/a.md".to_string(),
                ..EditorSurface::default()
            },
            EditorSurface {
                visible: vec!["/a.md".to_string()],
                ..EditorSurface::default()
            },
        ] {
            let (advanced, intent) = tracking.advance(&inert);
            assert_eq!(intent, SurfaceIntent::Idle);
            assert_eq!(advanced, tracking);
        }
    }

    #[test]
    fn the_signature_ignores_within_column_duplicates_but_not_column_order() {
        let doubled = surface("/a.md", &[&["/a.md", "/a.md"], &["/b.md"]]);
        let single = surface("/a.md", &[&["/a.md"], &["/b.md"]]);
        assert_eq!(doubled.visible_signature(), single.visible_signature());

        let swapped = surface("/a.md", &[&["/b.md"], &["/a.md"]]);
        assert_ne!(
            single.visible_signature(),
            swapped.visible_signature(),
            "the same documents in swapped columns are a different tmux layout"
        );
    }

    #[test]
    fn without_a_detected_layout_the_signature_is_the_sorted_visible_set() {
        let unordered = EditorSurface {
            focused: "/a.md".to_string(),
            visible: vec!["/b.md".to_string(), "/a.md".to_string(), "/b.md".to_string()],
            ..EditorSurface::default()
        };
        let reordered = EditorSurface {
            visible: vec!["/a.md".to_string(), "/b.md".to_string()],
            ..unordered.clone()
        };
        assert_eq!(
            unordered.visible_signature(),
            reordered.visible_signature(),
            "with no layout to mirror, only the visible SET is observable"
        );
    }
}
