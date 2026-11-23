//! GH #434: the notice a Run Agent Doc route carries when it dispatched but
//! left the tmux layout where it was.
//!
//! A route on flat editor evidence (`layout-detect windows=0
//! source=remote_client_selected_files reason=flat_evidence_has_no_column_geometry`)
//! has no column geometry to publish. The controller then either substitutes
//! the routed document alone (GH #373, the editor named no visible document)
//! or defers the structural effect and dispatches focus-only into the
//! document's existing pane (GH #347). Both are correct, but the route used to
//! answer only `[route] dispatched via controller editor_route`, which the
//! JetBrains plugin logged as `[route] SUCCESS`: the operator saw a successful
//! route and a tmux layout that never moved, with no logged reason.
//!
//! The route output now ends with one `[route] layout-not-synced:` line naming
//! why, so the editor log records the refusal beside the success.

/// Line prefix of [`route_layout_not_synced_notice`].
pub const ROUTE_LAYOUT_NOT_SYNCED_MARKER: &str = "[route] layout-not-synced:";

/// The receipt reason of a route layout the controller deferred to a
/// focus-only dispatch (GH #347).
pub const ROUTE_LAYOUT_DEFERRED_FOCUS_ONLY_REASON: &str = "layout_deferred_focus_only";

/// Why a dispatched route left the tmux layout unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteLayoutNotSynced {
    /// The editor's route named no visible document, so only the routed
    /// document was ensured a column (GH #373).
    EditorObservedNoDocument,
    /// No authoritative frontend snapshot exists, so the structural effect was
    /// deferred and the route dispatched focus-only (GH #347).
    DeferredFocusOnly,
}

impl RouteLayoutNotSynced {
    /// Classify one route: `editor_observed_no_document` is the GH #373
    /// substitution, `receipt_reason` the converged layout receipt's reason.
    /// A deferral wins: it is the stronger statement (nothing was published).
    pub fn classify(editor_observed_no_document: bool, receipt_reason: &str) -> Option<Self> {
        if receipt_reason == ROUTE_LAYOUT_DEFERRED_FOCUS_ONLY_REASON {
            Some(Self::DeferredFocusOnly)
        } else if editor_observed_no_document {
            Some(Self::EditorObservedNoDocument)
        } else {
            None
        }
    }

    /// Stable wire token for the ops log and the notice.
    pub const fn cause(self) -> &'static str {
        match self {
            Self::EditorObservedNoDocument => "editor_observed_no_document",
            Self::DeferredFocusOnly => "incomplete_frontend_layout_no_authoritative_snapshot",
        }
    }
}

/// The operator-facing notice of a route that dispatched `relative_path` but
/// did not move the tmux layout to the editor's visible documents.
pub fn route_layout_not_synced_notice(relative_path: &str, cause: RouteLayoutNotSynced) -> String {
    let effect = match cause {
        RouteLayoutNotSynced::EditorObservedNoDocument => format!(
            "the editor reported no visible document, so the route only made sure {relative_path} has a pane"
        ),
        RouteLayoutNotSynced::DeferredFocusOnly => format!(
            "the route sent the prompt to {relative_path}'s existing pane and left the tmux layout unchanged"
        ),
    };
    format!(
        "{ROUTE_LAYOUT_NOT_SYNCED_MARKER} {relative_path} reason=flat_evidence_has_no_column_geometry cause={}: \
         the editor sent no column geometry for this route; {effect}. The tmux columns follow the editor \
         again once it publishes its split layout (Remote Dev: {}).",
        cause.cause(),
        crate::FRONTEND_AUTHORITY_UNAVAILABLE_REMEDY,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RouteOutcomeEvidence, RouteOutcomeKind, derive_route_outcome};

    #[test]
    fn a_deferred_route_is_classified_before_the_substitution() {
        assert_eq!(
            RouteLayoutNotSynced::classify(true, ROUTE_LAYOUT_DEFERRED_FOCUS_ONLY_REASON),
            Some(RouteLayoutNotSynced::DeferredFocusOnly)
        );
        assert_eq!(
            RouteLayoutNotSynced::classify(false, ROUTE_LAYOUT_DEFERRED_FOCUS_ONLY_REASON),
            Some(RouteLayoutNotSynced::DeferredFocusOnly)
        );
        assert_eq!(
            RouteLayoutNotSynced::classify(true, "applied"),
            Some(RouteLayoutNotSynced::EditorObservedNoDocument)
        );
        assert_eq!(RouteLayoutNotSynced::classify(false, "applied"), None);
    }

    /// The notice rides a successful route's output. It must name the reason
    /// and never change what the editor reads the route as: a dispatched route
    /// stays a success, never queued, busy, or steering.
    #[test]
    fn the_notice_names_the_reason_and_keeps_the_route_a_success() {
        for cause in [
            RouteLayoutNotSynced::EditorObservedNoDocument,
            RouteLayoutNotSynced::DeferredFocusOnly,
        ] {
            let notice = route_layout_not_synced_notice("tasks/a.md", cause);
            assert!(
                notice.starts_with(ROUTE_LAYOUT_NOT_SYNCED_MARKER),
                "{notice}"
            );
            assert!(
                notice.contains("reason=flat_evidence_has_no_column_geometry"),
                "{notice}"
            );
            assert!(
                notice.contains(&format!("cause={}", cause.cause())),
                "{notice}"
            );
            assert!(!notice.contains('\n'), "one line: {notice}");
            let output =
                format!("[route] dispatched via controller editor_route for tasks/a.md\n{notice}");
            let outcome = derive_route_outcome(&RouteOutcomeEvidence {
                output: Some(output),
                relative_path: Some("tasks/a.md".to_string()),
            });
            assert_eq!(outcome.kind, RouteOutcomeKind::Persistent, "{notice}");
            assert!(outcome.pane_not_visible_notice.is_none());
            assert!(outcome.owned_by_other_lane_notice.is_none());
            assert_eq!(
                outcome.layout_not_synced_notice.as_deref(),
                Some(notice.as_str())
            );
        }
    }
}
