//! Editor commands that name the propagated surface instead of re-deriving it
//! (`specs/13-state-backbone.md` "State Propagation, Not Effect Inference").
//!
//! `Run Agent Doc` and `Sync Tmux Panes` used to read the editor split at click
//! time and send it as `--col a,b --col c` arguments. That is a snapshot of the
//! editor state taken by the command, a second source of truth beside the
//! surface `Source` the editor already publishes on every layout change, and
//! the two could disagree (GH #320 was one such disagreement).
//!
//! A command now publishes the current surface on the ordinary
//! `editor_surface_observe` path and then names that publication with a
//! [`SurfaceLayoutReference`]. The controller reads its retained `Source` for
//! the named `(client, surface)` endpoint and derives the command's layout from
//! it here, with the same column shape the automatic capture path uses.
//!
//! Everything in this module is a pure, total function of the retained value.
//! A missing, retired, or stale reference and an unusable surface are typed
//! refusals, never a guessed layout.

use serde::{Deserialize, Serialize};

use crate::{EditorSurface, SurfaceColumnOrder};

/// Advertised by a controller that resolves [`SurfaceLayoutReference`]s.
///
/// The controller returns it in every `editor_surface_observe` reply. An
/// editor publishes the surface first anyway, so the reply that proves its
/// fact is retained also proves the controller can resolve a reference to it.
/// A reply without the capability (an older binary) means the editor must
/// keep sending the explicit `--col` publication.
pub const SURFACE_LAYOUT_REFERENCE_CAPABILITY: &str = "surface_layout_reference_v1";

/// The repair for a Remote Dev backend that never receives frontend geometry.
///
/// Keep controller admission failures and editor-facing surface refusals on this
/// one phrase; the command-specific retry instruction may follow it.
pub const FRONTEND_AUTHORITY_UNAVAILABLE_REMEDY: &str = "install the same Agent Doc modular plugin ZIP in JetBrains Client/Gateway and the Remote Dev backend, verify both sides report that exact plugin version, restart both (including the JetBrains Client/Gateway process; restarting only the Remote Dev backend cannot replace its already-loaded frontend module)";

/// Readiness classification for a Remote Dev peer that has not established the
/// authenticated frontend surface lease after the startup grace period.
///
/// The backend cannot distinguish an absent frontend module from a module whose
/// generation cannot negotiate with it. Both are distribution/readiness faults,
/// not transient editor geometry and not permission to reconstruct columns.
pub const FRONTEND_AUTHORITY_UNAVAILABLE_READINESS: &str =
    "frontend_authority_absent_or_generation_mismatched";

/// Every editor-command capability this build of the controller advertises.
pub const CONTROLLER_SURFACE_CAPABILITIES: &[&str] = &[SURFACE_LAYOUT_REFERENCE_CAPABILITY];

/// The surface publication a command acts on.
///
/// The requesting client is the command's own `client_id`, which is the same
/// identity the editor publishes its surface as. `min_sequence` is the
/// sequence of the publication the editor made for this command: the retained
/// `Source` must be at least that new. A newer observation from the same
/// client is newer state and is used as is.
///
/// The comparison is by sequence alone, so it relies on the publisher's
/// sequence being monotonic across its connection generations. The JetBrains
/// plugin draws every surface observation from one process-wide counter, so a
/// re-registration never lowers it. A client that restarts its sequence per
/// generation must not send a reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceLayoutReference {
    pub surface_id: String,
    #[serde(default)]
    pub min_sequence: u64,
}

/// The controller's retained value for one `(client, surface)` endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetainedEditorSurface {
    pub surface: EditorSurface,
    pub generation: u64,
    pub sequence: u64,
    pub retired: bool,
}

/// Why a reference did not yield a layout. Each refusal leaves the retained
/// tmux layout untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceReferenceRefusal {
    /// The command did not name the client whose surface it references.
    MissingClient,
    /// This controller holds no surface for the named endpoint.
    Unavailable,
    /// The named endpoint was retired.
    Retired,
    /// The retained surface is older than the publication the command named.
    Stale,
    /// The surface has no focused or visible session document.
    Inert,
    /// The surface published its split as unknown (`columns=[]`), so it
    /// carries no structural authority.
    UnknownLayout,
    /// A foreground sync whose focused document is in none of the columns.
    StaleFocus,
}

impl SurfaceReferenceRefusal {
    /// The stable token, prefixed with `surface_reference_` on the wire.
    pub fn label(self) -> &'static str {
        match self {
            Self::MissingClient => "missing_client",
            Self::Unavailable => "unavailable",
            Self::Retired => "retired",
            Self::Stale => "stale",
            Self::Inert => "inert",
            Self::UnknownLayout => "unknown_layout",
            Self::StaleFocus => "stale_focus",
        }
    }

    /// The operator-facing sentence. The two layout refusals keep the exact
    /// wording the editor used when it made these decisions itself.
    pub fn message(self) -> &'static str {
        match self {
            Self::MissingClient => {
                "The command named an editor surface but not the editor client that published it."
            }
            Self::Unavailable => {
                "The project controller holds no editor surface for this editor; the retained tmux layout was not changed."
            }
            Self::Retired => {
                "The editor surface this command named was retired; the retained tmux layout was not changed."
            }
            Self::Stale => {
                "The project controller has not yet retained the editor surface this command published; retry."
            }
            Self::Inert => "No agent-doc session document focused",
            Self::UnknownLayout => {
                "Remote Dev frontend structural routing is not ready (frontend_authority_absent_or_generation_mismatched; flat_evidence_has_no_column_geometry); the retained tmux layout was not changed. Remedy: install the same Agent Doc modular plugin ZIP in JetBrains Client/Gateway and the Remote Dev backend, verify both sides report that exact plugin version, restart both (including the JetBrains Client/Gateway process; restarting only the Remote Dev backend cannot replace its already-loaded frontend module), then retry Sync Tmux Layout."
            }
            Self::StaleFocus => {
                "Editor focus changed while the split layout was being read; retry after the selection settles."
            }
        }
    }

    /// `surface_reference_<label>: <message>`, the form a controller error carries.
    pub fn diagnostic(self) -> String {
        format!("surface_reference_{}: {}", self.label(), self.message())
    }
}

/// Resolve a reference against the controller's retained value.
pub fn resolve_surface_reference<'a>(
    retained: Option<&'a RetainedEditorSurface>,
    reference: &SurfaceLayoutReference,
) -> Result<&'a EditorSurface, SurfaceReferenceRefusal> {
    let retained = retained.ok_or(SurfaceReferenceRefusal::Unavailable)?;
    if retained.retired {
        return Err(SurfaceReferenceRefusal::Retired);
    }
    if retained.sequence < reference.min_sequence {
        return Err(SurfaceReferenceRefusal::Stale);
    }
    Ok(&retained.surface)
}

/// The surface's columns in the shape every layout publication uses: one
/// comma-joined entry per column, blank paths and empty columns dropped.
/// This is the same shape the automatic surface `Sync` intent publishes.
fn column_entries(surface: &EditorSurface) -> Vec<String> {
    surface
        .sync_columns()
        .iter()
        .map(|column| {
            column
                .files
                .iter()
                .map(|file| file.trim())
                .filter(|file| !file.is_empty())
                .collect::<Vec<_>>()
                .join(",")
        })
        .filter(|column| !column.is_empty())
        .collect()
}

/// Layout of an explicit `sync_tmux_layout` derived from the surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceSyncLayout {
    pub columns: Vec<String>,
    pub column_order: SurfaceColumnOrder,
    pub focus: String,
}

/// An explicit sync is exact-visible structural authority, so it needs a
/// known split: an unknown one is refused rather than replaced by a fallback.
/// `foreground` syncs also own pane focus, so their focused document must be
/// one of the columns.
pub fn sync_layout_from_surface(
    surface: &EditorSurface,
    foreground: bool,
) -> Result<SurfaceSyncLayout, SurfaceReferenceRefusal> {
    if surface.is_inert() {
        return Err(SurfaceReferenceRefusal::Inert);
    }
    let columns = column_entries(surface);
    if columns.is_empty() {
        return Err(SurfaceReferenceRefusal::UnknownLayout);
    }
    let focus = surface.focused.trim().to_string();
    if foreground
        && !columns
            .iter()
            .any(|column| column.split(',').any(|file| file == focus))
    {
        return Err(SurfaceReferenceRefusal::StaleFocus);
    }
    Ok(SurfaceSyncLayout {
        columns,
        column_order: surface.column_order,
        focus,
    })
}

/// `layout_mode` of a route whose columns are the whole visible split.
pub const ROUTE_LAYOUT_MODE_EXACT: &str = "exact";
/// `layout_mode` of a route that only asks each named document for a column.
pub const ROUTE_LAYOUT_MODE_ENSURE: &str = "ensure";

/// Layout of an `editor_route` derived from the surface, in the route's
/// `--col` argument form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceRouteLayout {
    pub layout_args: Vec<String>,
    pub column_order: SurfaceColumnOrder,
    pub layout_mode: &'static str,
}

/// GH #111: only a multi-column split is a positive observation of the whole
/// visible layout (`exact`). A single or unknown split names just the visible
/// documents, each in its own column (GH #81), and asks only that each has a
/// column (`ensure`); their order is not geometry, so it is `unknown` and the
/// controller keeps the order it retains.
pub fn route_layout_from_surface(
    surface: &EditorSurface,
) -> Result<SurfaceRouteLayout, SurfaceReferenceRefusal> {
    if surface.is_inert() {
        return Err(SurfaceReferenceRefusal::Inert);
    }
    let columns = column_entries(surface);
    let (columns, column_order, layout_mode) = if columns.len() > 1 {
        (columns, surface.column_order, ROUTE_LAYOUT_MODE_EXACT)
    } else {
        let mut visible: Vec<String> = Vec::new();
        for file in surface.visible.iter().map(|file| file.trim()) {
            if !file.is_empty() && !visible.iter().any(|seen| seen == file) {
                visible.push(file.to_string());
            }
        }
        (
            visible,
            SurfaceColumnOrder::Unknown,
            ROUTE_LAYOUT_MODE_ENSURE,
        )
    };
    let mut layout_args = Vec::with_capacity(columns.len() * 2 + 2);
    for column in columns {
        layout_args.push("--col".to_string());
        layout_args.push(column);
    }
    layout_args.push("--focus".to_string());
    layout_args.push(surface.focused.trim().to_string());
    Ok(SurfaceRouteLayout {
        layout_args,
        column_order,
        layout_mode,
    })
}

/// `layout_authority` of an `editor_route` whose editor deferred its own
/// surface because the JetBrains Client's measured geometry owns the Remote
/// Dev layout (GH #320 option B, `frontend_measured_layout_authority`). The
/// route then carries no column; the measured columns arrive separately on
/// the installation's [`REMOTE_CLIENT_MEASURED_SURFACE_FAMILY`] lane (GH #376).
pub const ROUTE_LAYOUT_AUTHORITY_FRONTEND_MEASURED: &str = "frontend_measured";

/// The surface family a Remote Dev backend forwards the JetBrains Client's
/// measured main-window columns as (`jetbrains-rd@<installation>-pid:<pid>`).
pub const REMOTE_CLIENT_MEASURED_SURFACE_FAMILY: &str = "jetbrains-rd";

/// GH #376: whether `client_id` is the measured-layout forward of the editor
/// `installation` (the same installation, the measured family).
pub fn is_measured_layout_client_of(client_id: &str, installation: &str) -> bool {
    client_id
        .split_once('@')
        .is_some_and(|(family, _)| family == REMOTE_CLIENT_MEASURED_SURFACE_FAMILY)
        && crate::editor_installation(client_id) == Some(installation)
}

/// GH #376: the layout a retained measured surface gives an editor route that
/// observed no column, or `None` when that surface cannot answer it.
///
/// It answers only while it is live, carries a known split measured in editor
/// order, and holds the routed document (`holds_routed` compares one surface
/// path with it). A measurement that does not show the routed document is an
/// older layout than the one the operator acted on, so it is not used.
pub fn measured_route_layout(
    retained: &RetainedEditorSurface,
    holds_routed: impl Fn(&str) -> bool,
) -> Option<SurfaceRouteLayout> {
    let surface = &retained.surface;
    if retained.retired
        || surface.column_order != SurfaceColumnOrder::Editor
        || column_entries(surface).is_empty()
    {
        return None;
    }
    let shows_routed = surface
        .sync_columns()
        .iter()
        .flat_map(|column| column.files.iter())
        .chain(surface.visible.iter())
        .map(|file| file.trim())
        .any(|file| !file.is_empty() && holds_routed(file));
    if !shows_routed {
        return None;
    }
    route_layout_from_surface(surface).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SurfaceColumn;

    fn surface(columns: &[&[&str]], visible: &[&str], focused: &str) -> EditorSurface {
        EditorSurface {
            focused: focused.to_string(),
            visible: visible.iter().map(|file| file.to_string()).collect(),
            open: Vec::new(),
            columns: columns
                .iter()
                .map(|files| SurfaceColumn::new(files.iter().copied()))
                .collect(),
            column_order: SurfaceColumnOrder::Editor,
            force_reconcile: false,
            focus_only: false,
            preserve_focus: false,
        }
    }

    fn retained(surface: EditorSurface, sequence: u64, retired: bool) -> RetainedEditorSurface {
        RetainedEditorSurface {
            surface,
            generation: 7,
            sequence,
            retired,
        }
    }

    #[test]
    fn reference_resolves_only_a_live_retained_surface_at_least_as_new() {
        let current = retained(surface(&[&["/p/a.md"]], &["/p/a.md"], "/p/a.md"), 5, false);
        let reference = SurfaceLayoutReference {
            surface_id: "frame-1".to_string(),
            min_sequence: 5,
        };
        assert_eq!(
            resolve_surface_reference(Some(&current), &reference),
            Ok(&current.surface)
        );
        // A newer publication from the same client is newer state, not a conflict.
        let newer = SurfaceLayoutReference {
            min_sequence: 3,
            ..reference.clone()
        };
        assert!(resolve_surface_reference(Some(&current), &newer).is_ok());
        let ahead = SurfaceLayoutReference {
            min_sequence: 6,
            ..reference.clone()
        };
        assert_eq!(
            resolve_surface_reference(Some(&current), &ahead),
            Err(SurfaceReferenceRefusal::Stale)
        );
        let gone = retained(current.surface.clone(), 9, true);
        assert_eq!(
            resolve_surface_reference(Some(&gone), &reference),
            Err(SurfaceReferenceRefusal::Retired)
        );
        assert_eq!(
            resolve_surface_reference(None, &reference),
            Err(SurfaceReferenceRefusal::Unavailable)
        );
    }

    #[test]
    fn sync_derives_the_same_column_shape_as_the_automatic_capture() {
        let observed = surface(
            &[&["/p/b.md"], &["/p/a.md", "/p/c.md"]],
            &["/p/a.md", "/p/b.md", "/p/c.md"],
            "/p/a.md",
        );
        let layout = sync_layout_from_surface(&observed, true).unwrap();
        let automatic: Vec<String> = observed
            .sync_columns()
            .iter()
            .map(|column| column.files.join(","))
            .collect();
        assert_eq!(layout.columns, automatic);
        assert_eq!(layout.columns, vec!["/p/b.md", "/p/a.md,/p/c.md"]);
        assert_eq!(layout.focus, "/p/a.md");
        assert_eq!(layout.column_order, SurfaceColumnOrder::Editor);
    }

    #[test]
    fn sync_carries_the_published_order_label_never_upgrading_it() {
        for order in [SurfaceColumnOrder::Unknown, SurfaceColumnOrder::Retained] {
            let mut observed = surface(
                &[&["/p/a.md"], &["/p/b.md"]],
                &["/p/a.md", "/p/b.md"],
                "/p/b.md",
            );
            observed.column_order = order;
            assert_eq!(
                sync_layout_from_surface(&observed, true)
                    .unwrap()
                    .column_order,
                order
            );
        }
    }

    #[test]
    fn sync_refuses_an_unknown_split_instead_of_guessing_one() {
        let unknown = surface(&[], &["/p/a.md", "/p/b.md"], "/p/a.md");
        assert_eq!(
            sync_layout_from_surface(&unknown, true),
            Err(SurfaceReferenceRefusal::UnknownLayout)
        );
        let blank = surface(&[&[" "]], &["/p/a.md"], "/p/a.md");
        assert_eq!(
            sync_layout_from_surface(&blank, false),
            Err(SurfaceReferenceRefusal::UnknownLayout)
        );
        let inert = surface(&[&["/p/a.md"]], &[], "");
        assert_eq!(
            sync_layout_from_surface(&inert, true),
            Err(SurfaceReferenceRefusal::Inert)
        );
    }

    #[test]
    fn foreground_sync_refuses_a_focus_outside_the_columns() {
        let drifted = surface(
            &[&["/p/a.md"], &["/p/b.md"]],
            &["/p/a.md", "/p/b.md"],
            "/p/c.md",
        );
        assert_eq!(
            sync_layout_from_surface(&drifted, true),
            Err(SurfaceReferenceRefusal::StaleFocus)
        );
        // A background (no-autostart) sync owns membership, not focus.
        assert!(sync_layout_from_surface(&drifted, false).is_ok());
    }

    #[test]
    fn route_publishes_a_multi_column_split_exactly_with_its_order_label() {
        let mut observed = surface(
            &[&["/p/b.md"], &["/p/a.md"]],
            &["/p/a.md", "/p/b.md"],
            "/p/a.md",
        );
        observed.column_order = SurfaceColumnOrder::Unknown;
        let layout = route_layout_from_surface(&observed).unwrap();
        assert_eq!(
            layout.layout_args,
            vec!["--col", "/p/b.md", "--col", "/p/a.md", "--focus", "/p/a.md"]
        );
        assert_eq!(layout.layout_mode, ROUTE_LAYOUT_MODE_EXACT);
        assert_eq!(layout.column_order, SurfaceColumnOrder::Unknown);
    }

    #[test]
    fn route_without_a_split_only_ensures_each_visible_document() {
        for columns in [&[][..], &[&["/p/a.md"][..]][..]] {
            let observed = surface(columns, &["/p/a.md", "/p/b.md", "/p/a.md"], "/p/a.md");
            let layout = route_layout_from_surface(&observed).unwrap();
            assert_eq!(
                layout.layout_args,
                vec!["--col", "/p/a.md", "--col", "/p/b.md", "--focus", "/p/a.md"]
            );
            assert_eq!(layout.layout_mode, ROUTE_LAYOUT_MODE_ENSURE);
            assert_eq!(layout.column_order, SurfaceColumnOrder::Unknown);
        }
    }

    #[test]
    fn refusal_diagnostics_carry_a_stable_token() {
        let diagnostic = SurfaceReferenceRefusal::UnknownLayout.diagnostic();
        assert_eq!(
            diagnostic,
            "surface_reference_unknown_layout: Remote Dev frontend structural routing is not ready (frontend_authority_absent_or_generation_mismatched; flat_evidence_has_no_column_geometry); the retained tmux layout was not changed. Remedy: install the same Agent Doc modular plugin ZIP in JetBrains Client/Gateway and the Remote Dev backend, verify both sides report that exact plugin version, restart both (including the JetBrains Client/Gateway process; restarting only the Remote Dev backend cannot replace its already-loaded frontend module), then retry Sync Tmux Layout."
        );
        assert!(diagnostic.contains(FRONTEND_AUTHORITY_UNAVAILABLE_REMEDY));
        assert!(diagnostic.contains(FRONTEND_AUTHORITY_UNAVAILABLE_READINESS));
        assert!(
            SurfaceReferenceRefusal::StaleFocus
                .diagnostic()
                .starts_with("surface_reference_stale_focus: ")
        );
    }

    /// GH #376: only the same installation's measured forward is its measured
    /// layout; the classic, focus, and another installation's families are not.
    #[test]
    fn measured_layout_client_is_the_same_installations_forward_family() {
        let installation = "IU.0a1b2c3d";
        assert!(is_measured_layout_client_of(
            "jetbrains-rd@IU.0a1b2c3d-pid:41",
            installation
        ));
        for other in [
            "jetbrains@IU.0a1b2c3d-pid:41",
            "jetbrains-focus@IU.0a1b2c3d-pid:41",
            "jetbrains-rd@IU.ffffffff-pid:41",
            "jetbrains-rd@IU.0a1b2c3d",
            "jetbrains-rd",
        ] {
            assert!(
                !is_measured_layout_client_of(other, installation),
                "{other}"
            );
        }
    }

    /// GH #376: a live, editor-ordered measured split that shows the routed
    /// document is the route's exact layout; anything else cannot answer it.
    #[test]
    fn measured_route_layout_needs_a_live_editor_ordered_split_showing_the_routed_document() {
        let measured = surface(
            &[&["/p/b.md"], &["/p/a.md"]],
            &["/p/b.md", "/p/a.md"],
            "/p/b.md",
        );
        let is_a = |file: &str| file == "/p/a.md";
        let layout = measured_route_layout(&retained(measured.clone(), 4, false), is_a)
            .expect("the measured split answers the route");
        assert_eq!(
            layout.layout_args,
            vec!["--col", "/p/b.md", "--col", "/p/a.md", "--focus", "/p/b.md"]
        );
        assert_eq!(layout.layout_mode, ROUTE_LAYOUT_MODE_EXACT);
        assert_eq!(layout.column_order, SurfaceColumnOrder::Editor);

        assert_eq!(
            measured_route_layout(&retained(measured.clone(), 4, true), is_a),
            None,
            "a retired forward is not the current layout"
        );
        assert_eq!(
            measured_route_layout(&retained(measured.clone(), 4, false), |file: &str| {
                file == "/p/c.md"
            }),
            None,
            "a measurement that does not show the routed document predates the route"
        );
        let unknown_order = EditorSurface {
            column_order: SurfaceColumnOrder::Unknown,
            ..measured.clone()
        };
        assert_eq!(
            measured_route_layout(&retained(unknown_order, 4, false), is_a),
            None,
            "flat membership order is not a measurement"
        );
        let unknown_split = surface(&[], &["/p/b.md", "/p/a.md"], "/p/a.md");
        assert_eq!(
            measured_route_layout(&retained(unknown_split, 4, false), is_a),
            None,
            "an unknown split carries no structure"
        );
    }

    #[test]
    fn reference_wire_shape_defaults_the_sequence() {
        let reference: SurfaceLayoutReference =
            serde_json::from_str(r#"{"surface_id":"frame-1"}"#).unwrap();
        assert_eq!(reference.min_sequence, 0);
    }
}
