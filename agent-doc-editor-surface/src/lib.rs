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
    /// Every open markdown document owned by this editor process, ordered by
    /// editor proximity: focused first, then nearby tabs, then the remaining
    /// open documents. This may be larger than `visible`; editor adapters may
    /// use it to choose which controller projections to subscribe to.
    #[serde(default)]
    pub open: Vec<String>,
    /// The split layout. Empty means "layout not detected"; the signature then
    /// falls back to the sorted visible set.
    #[serde(default)]
    pub columns: Vec<SurfaceColumn>,
    /// The operator asked for a reconcile explicitly, so skip the unchanged-
    /// observation shortcut.
    #[serde(default)]
    pub force_reconcile: bool,
    /// This observation belongs to the selected document's controller and owns
    /// only pane selection. It must never infer a structural layout sync from
    /// its intentionally narrow one-document payload.
    #[serde(default)]
    pub focus_only: bool,
}

/// Ordered editor fact sent to the Project Controller.
///
/// The transport is request/response framed, but this value is observation
/// ingress: the editor reports facts and never chooses the resulting intent.
/// `(client_id, generation, sequence)` lets the controller reject replay from
/// a retired native generation after an editor/plugin reload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorSurfaceObservation {
    pub client_id: String,
    pub generation: u64,
    pub sequence: u64,
    pub surface: EditorSurface,
}

/// Availability of the controller-owned turn projection for one document.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentAuthorityReadiness {
    /// The editor reported the document and has not received its first
    /// controller projection yet.
    #[default]
    Pending,
    /// The controller-owned projection was read successfully.
    Ready,
    /// The editor could not resolve or read the controller projection.
    Unavailable,
}

/// Controller projection for one open document.
///
/// This is a Source value. Editors do not manufacture it: they observe it from
/// the controller independently for every open document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentAuthority {
    pub document: String,
    pub readiness: DocumentAuthorityReadiness,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<agent_doc_turn::cp_projection::TurnProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub revision: u64,
}

impl DocumentAuthority {
    pub fn pending(document: impl Into<String>) -> Self {
        Self {
            document: document.into(),
            readiness: DocumentAuthorityReadiness::Pending,
            turn: None,
            error: None,
            revision: 0,
        }
    }
}

/// Pure join of the selected editor document and that document's controller
/// authority.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentDocumentAuthority {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authority: Option<DocumentAuthority>,
}

/// The column layout tmux is actually showing, as observed by the controller.
///
/// The counterpart to [`EditorSurface`]. Both sides of the mirror are now
/// observations in the same graph, which is what lets "has tmux drifted?" be a
/// *derivation* rather than a field.
///
/// It used to be a field: `EditorSurface::layout_synced`, reported by the plugin.
/// That asked the editor for a fact only the controller has — so a plugin either
/// left it unset, in which case proven drift never reconciled, or paid a round
/// trip to the controller to learn it before reporting it back. Deriving it also
/// means tmux drifting is an event in its own right: the controller writes its
/// observation and the consequence follows, with no editor event needed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TmuxLayout {
    #[serde(default)]
    pub columns: Vec<SurfaceColumn>,
}

impl TmuxLayout {
    pub fn signature(&self) -> String {
        column_signature(&self.columns)
    }
}

/// A stable identity for a column layout: which documents, arranged how.
///
/// Shared by both sides of the mirror so the comparison cannot drift — an editor
/// signature and a tmux signature are computed by the same function or they are
/// not comparable.
fn column_signature(columns: &[SurfaceColumn]) -> String {
    columns
        .iter()
        .map(|column| {
            let mut seen: Vec<&String> = Vec::new();
            for file in &column.files {
                if !file.is_empty() && !seen.contains(&file) {
                    seen.push(file);
                }
            }
            seen.into_iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(&FILE_SEPARATOR.to_string())
        })
        .collect::<Vec<_>>()
        .join(&COLUMN_SEPARATOR.to_string())
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
            return column_signature(&self.columns);
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

/// Whether tmux matches the layout the editor is showing.
///
/// Three-valued on purpose. `None` is "the controller has not reported a tmux
/// layout", and it must not read as "it has drifted" — that is the same
/// inversion that had the supervisor's idle watch answer an unresponsive
/// controller with its *expensive* probes, up to 120x the intended load. Only
/// `Some(false)` is evidence of drift.
///
/// The editor side is compared column-wise only when the editor detected a
/// layout; with no detected layout there is nothing to mirror, so the answer is
/// unknown rather than "mismatched".
pub fn layout_matches(surface: &EditorSurface, tmux: Option<&TmuxLayout>) -> Option<bool> {
    let tmux = tmux?;
    if surface.columns.is_empty() {
        return None;
    }
    Some(column_signature(&surface.columns) == tmux.signature())
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

/// Projection returned after the controller folds an editor observation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurfaceObservationReceipt {
    pub intent: SurfaceIntent,
    /// `true` when the observation implied no tmux consequence.
    pub idle: bool,
    /// The consequence's reply, when one ran.
    pub outcome: Option<String>,
    /// A consequence failure does not invalidate the editor fact.
    pub error: Option<String>,
}

/// Controller-published reactive projection for an accepted editor fact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorSurfaceProjection {
    pub client_id: String,
    pub generation: u64,
    pub sequence: u64,
    pub receipt: SurfaceObservationReceipt,
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
    /// A controller probe is useful only when the editor layout is unchanged.
    /// A first or structurally-new layout already implies `Sync`, so probing
    /// tmux before publishing that intent adds a round trip without changing the
    /// decision. Repeated layouts still probe so controller-observed drift can
    /// turn an otherwise-idle/focus observation into a reconcile.
    pub fn requires_tmux_probe(&self, surface: &EditorSurface) -> bool {
        if surface.force_reconcile || surface.is_inert() {
            return false;
        }
        let signature = surface.visible_signature();
        self.reconciled_signature.as_deref() == Some(signature.as_str())
    }

    /// Fold `surface` into this tracking value, returning the next tracking
    /// value and the intent the observation implies.
    ///
    /// Total: every observation produces an answer, and an observation that
    /// implies nothing produces [`SurfaceIntent::Idle`] with the tracking value
    /// unchanged. Advancing on an idle observation is what would make a
    /// duplicate event look like a change on the next one.
    pub fn advance(
        &self,
        surface: &EditorSurface,
        layout_matches: Option<bool>,
    ) -> (Self, SurfaceIntent) {
        if surface.is_inert() {
            return (self.clone(), SurfaceIntent::Idle);
        }

        let same_focus = self.focused_document.as_deref() == Some(surface.focused.as_str());
        if surface.focus_only {
            if !surface.force_reconcile && same_focus {
                return (self.clone(), SurfaceIntent::Idle);
            }
            return (
                Self {
                    reconciled_signature: self.reconciled_signature.clone(),
                    focused_document: Some(surface.focused.clone()),
                },
                SurfaceIntent::Focus {
                    document: surface.focused.clone(),
                },
            );
        }

        let drifted = layout_matches == Some(false);
        let signature = surface.visible_signature();
        let same_layout = self.reconciled_signature.as_deref() == Some(signature.as_str());

        // Nothing observable changed and tmux is not known to have drifted.
        if !surface.force_reconcile && !drifted && same_layout && same_focus {
            return (self.clone(), SurfaceIntent::Idle);
        }

        let advanced = Self {
            reconciled_signature: Some(signature),
            focused_document: Some(surface.focused.clone()),
        };

        // tmux has drifted from a layout the editor never changed: focusing
        // would select the right document in the wrong column, so reconcile.
        if drifted {
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
            .collect::<Vec<_>>();
        EditorSurface {
            focused: focused.to_string(),
            open: visible.clone(),
            visible,
            columns,
            force_reconcile: false,
            focus_only: false,
        }
    }

    /// The default in these tests: the controller has reported a tmux layout
    /// that matches. Drift is opted into per-test.
    const MATCHES: Option<bool> = Some(true);

    #[test]
    fn the_first_observation_reconciles_the_layout() {
        let (tracking, intent) = SurfaceTracking::default()
            .advance(&surface("/a.md", &[&["/a.md"], &["/b.md"]]), MATCHES);
        assert!(matches!(intent, SurfaceIntent::Sync { .. }));
        assert_eq!(intent.document(), Some("/a.md"));
        assert_eq!(tracking.focused_document.as_deref(), Some("/a.md"));
    }

    #[test]
    fn repeating_the_same_observation_implies_nothing() {
        let observed = surface("/a.md", &[&["/a.md"], &["/b.md"]]);
        let (tracking, _) = SurfaceTracking::default().advance(&observed, MATCHES);
        let (again, intent) = tracking.advance(&observed, MATCHES);
        assert_eq!(intent, SurfaceIntent::Idle);
        assert_eq!(
            again, tracking,
            "an idle observation must not advance the tracking value"
        );
    }

    #[test]
    fn only_an_unchanged_layout_requires_a_tmux_probe() {
        let first = surface("/a.md", &[&["/a.md"], &["/b.md"]]);
        let changed = surface("/c.md", &[&["/a.md"], &["/c.md"]]);
        let tracking = SurfaceTracking::default().advance(&first, None).0;

        assert!(tracking.requires_tmux_probe(&first));
        assert!(!tracking.requires_tmux_probe(&changed));
        assert!(!SurfaceTracking::default().requires_tmux_probe(&first));
    }

    #[test]
    fn moving_focus_within_an_unchanged_layout_is_a_focus_move() {
        let (tracking, _) = SurfaceTracking::default()
            .advance(&surface("/a.md", &[&["/a.md"], &["/b.md"]]), MATCHES);
        let (_, intent) = tracking.advance(&surface("/b.md", &[&["/a.md"], &["/b.md"]]), MATCHES);
        assert_eq!(
            intent,
            SurfaceIntent::Focus {
                document: "/b.md".to_string()
            },
            "layout reconciliation never moves the active pane, so a doc switch must focus"
        );
    }

    #[test]
    fn focus_only_observation_never_derives_a_layout_sync() {
        let focused = EditorSurface {
            focused: "/submodule/task.md".to_string(),
            visible: vec!["/submodule/task.md".to_string()],
            open: vec!["/submodule/task.md".to_string()],
            force_reconcile: true,
            focus_only: true,
            ..EditorSurface::default()
        };
        let (tracking, intent) = SurfaceTracking::default().advance(&focused, Some(false));
        assert_eq!(
            intent,
            SurfaceIntent::Focus {
                document: "/submodule/task.md".to_string(),
            },
        );
        assert_eq!(
            tracking.focused_document.as_deref(),
            Some("/submodule/task.md")
        );
        assert_eq!(
            tracking.reconciled_signature, None,
            "selection-only state must not claim a layout was reconciled"
        );
    }

    #[test]
    fn a_changed_layout_reconciles_even_with_unchanged_focus() {
        let (tracking, _) = SurfaceTracking::default()
            .advance(&surface("/a.md", &[&["/a.md"], &["/b.md"]]), MATCHES);
        let (_, intent) = tracking.advance(&surface("/a.md", &[&["/a.md"]]), MATCHES);
        assert!(matches!(intent, SurfaceIntent::Sync { .. }));
    }

    #[test]
    fn proven_tmux_drift_reconciles_an_otherwise_unchanged_surface() {
        let observed = surface("/a.md", &[&["/a.md"], &["/b.md"]]);
        let (tracking, _) = SurfaceTracking::default().advance(&observed, MATCHES);
        let (_, intent) = tracking.advance(&observed, Some(false));
        assert!(
            matches!(intent, SurfaceIntent::Sync { .. }),
            "focusing would select the right document in the wrong column"
        );
    }

    #[test]
    fn an_unknown_layout_state_is_not_read_as_drift() {
        let observed = surface("/a.md", &[&["/a.md"], &["/b.md"]]);
        let (tracking, _) = SurfaceTracking::default().advance(&observed, MATCHES);
        let (_, intent) = tracking.advance(&observed, None);
        assert_eq!(
            intent,
            SurfaceIntent::Idle,
            "\"the controller has not reported a tmux layout\" must stay distinct from \"it has drifted\""
        );
    }

    #[test]
    fn the_mirror_comparison_is_derived_from_both_sides() {
        let editor = surface("/a.md", &[&["/a.md"], &["/b.md"]]);
        let same = TmuxLayout {
            columns: editor.columns.clone(),
        };
        let swapped = TmuxLayout {
            columns: vec![SurfaceColumn::new(["/b.md"]), SurfaceColumn::new(["/a.md"])],
        };

        assert_eq!(layout_matches(&editor, Some(&same)), Some(true));
        assert_eq!(
            layout_matches(&editor, Some(&swapped)),
            Some(false),
            "the same documents in swapped columns is drift, not a match"
        );
        assert_eq!(
            layout_matches(&editor, None),
            None,
            "no reported tmux layout is unknown, never drift"
        );

        let no_detected_layout = EditorSurface {
            columns: Vec::new(),
            ..editor
        };
        assert_eq!(
            layout_matches(&no_detected_layout, Some(&same)),
            None,
            "with no editor layout there is nothing to mirror, so the answer is unknown"
        );
    }

    #[test]
    fn force_reconcile_re_emits_but_does_not_change_the_kind() {
        let observed = surface("/a.md", &[&["/a.md"], &["/b.md"]]);
        let (tracking, _) = SurfaceTracking::default().advance(&observed, MATCHES);
        let forced = EditorSurface {
            force_reconcile: true,
            ..observed
        };
        let (_, intent) = tracking.advance(&forced, MATCHES);
        // `force_reconcile` suppresses the unchanged-observation shortcut; it
        // does not claim the layout drifted. Only a derived `Some(false)` is
        // evidence of that, and inventing a sync here would neutralize the
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
        let (_, intent) = tracking.advance(&forced_new_layout, MATCHES);
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
            let (advanced, intent) = tracking.advance(&inert, MATCHES);
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
            visible: vec![
                "/b.md".to_string(),
                "/a.md".to_string(),
                "/b.md".to_string(),
            ],
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
