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

/// What the route's own payload proved about its missing column geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteLayoutEvidence {
    /// The route named no document and declared no frontend authority: the
    /// flat Remote Dev `--col` fallback (GH #373), whose remedy is installing
    /// the frontend module in the JetBrains Client.
    FlatNoDocument,
    /// The route declared `layout_authority=frontend_measured`: a JetBrains
    /// Client frontend module is installed and connected (GH #376), but the
    /// controller held no measurement for this route (after a controller or
    /// Client restart, while a forward is late, or while the backend keeps
    /// refusing that Client's snapshots). The authority proves a connected
    /// frontend, not a matching build, so the notice names the refusal and
    /// mismatch signs that do call for reinstalling.
    FrontendMeasurementNotReceived,
    /// The route named its documents in an unknown column order, which a
    /// one-sided Remote Dev backend and a local single-split editor both send:
    /// the controller cannot tell them apart, so the notice claims neither.
    ColumnOrderUnknown,
}

impl RouteLayoutEvidence {
    /// Classify the route's payload. A declared frontend authority wins: it is
    /// positive proof that the Client module is installed.
    pub const fn of_route(editor_observed_no_document: bool, frontend_measured: bool) -> Self {
        if frontend_measured {
            Self::FrontendMeasurementNotReceived
        } else if editor_observed_no_document {
            Self::FlatNoDocument
        } else {
            Self::ColumnOrderUnknown
        }
    }

    /// Stable reason token for the ops log and the notice.
    pub const fn reason(self) -> &'static str {
        match self {
            Self::FlatNoDocument => ROUTE_LAYOUT_FLAT_EVIDENCE_REASON,
            Self::FrontendMeasurementNotReceived => {
                ROUTE_LAYOUT_FRONTEND_MEASUREMENT_NOT_RECEIVED_REASON
            }
            Self::ColumnOrderUnknown => ROUTE_LAYOUT_COLUMN_ORDER_UNKNOWN_REASON,
        }
    }
}

/// Why a dispatched route left the tmux layout unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteLayoutNotSynced {
    /// The editor's route named no visible document, so only the routed
    /// document was ensured a column (GH #373).
    EditorObservedNoDocument { evidence: RouteLayoutEvidence },
    /// No authoritative frontend snapshot exists, so the structural effect was
    /// deferred and the route dispatched focus-only (GH #347).
    DeferredFocusOnly { evidence: RouteLayoutEvidence },
}

/// Reason token of a route whose own evidence was the flat Remote Dev fallback.
pub const ROUTE_LAYOUT_FLAT_EVIDENCE_REASON: &str = "flat_evidence_has_no_column_geometry";

/// Reason token of a route that declared the Client's measured layout the
/// controller had not received.
pub const ROUTE_LAYOUT_FRONTEND_MEASUREMENT_NOT_RECEIVED_REASON: &str =
    "frontend_measurement_not_received";

/// Reason token of a route that named its documents but no column order.
pub const ROUTE_LAYOUT_COLUMN_ORDER_UNKNOWN_REASON: &str = "route_column_order_unknown";

impl RouteLayoutNotSynced {
    /// Classify one route: `editor_observed_no_document` is the GH #373
    /// substitution, `frontend_measured` whether the route declared
    /// `layout_authority=frontend_measured`, `receipt_reason` the converged
    /// layout receipt's reason. A deferral wins: it is the stronger statement
    /// (nothing was published).
    pub fn classify(
        editor_observed_no_document: bool,
        frontend_measured: bool,
        receipt_reason: &str,
    ) -> Option<Self> {
        let evidence =
            RouteLayoutEvidence::of_route(editor_observed_no_document, frontend_measured);
        if receipt_reason == ROUTE_LAYOUT_DEFERRED_FOCUS_ONLY_REASON {
            Some(Self::DeferredFocusOnly { evidence })
        } else if editor_observed_no_document {
            Some(Self::EditorObservedNoDocument { evidence })
        } else {
            None
        }
    }

    /// Stable wire token for the ops log and the notice.
    pub const fn cause(self) -> &'static str {
        match self {
            Self::EditorObservedNoDocument { .. } => "editor_observed_no_document",
            Self::DeferredFocusOnly { .. } => {
                "incomplete_frontend_layout_no_authoritative_snapshot"
            }
        }
    }

    /// What the route's own payload proved.
    pub const fn evidence(self) -> RouteLayoutEvidence {
        match self {
            Self::EditorObservedNoDocument { evidence } | Self::DeferredFocusOnly { evidence } => {
                evidence
            }
        }
    }

    /// Stable reason token: what the route's own evidence was.
    pub const fn reason(self) -> &'static str {
        self.evidence().reason()
    }
}

/// The operator-facing notice of a route that dispatched `relative_path` but
/// did not move the tmux layout to the editor's visible documents.
pub fn route_layout_not_synced_notice(relative_path: &str, cause: RouteLayoutNotSynced) -> String {
    let remedy = crate::FRONTEND_AUTHORITY_UNAVAILABLE_REMEDY;
    let effect = match cause {
        RouteLayoutNotSynced::EditorObservedNoDocument { .. } => {
            format!("the route only made sure {relative_path} has a pane")
        }
        RouteLayoutNotSynced::DeferredFocusOnly { .. } => format!(
            "the route sent the prompt to {relative_path}'s existing pane and left the tmux layout unchanged"
        ),
    };
    let detail = match cause.evidence() {
        RouteLayoutEvidence::FlatNoDocument => format!(
            "the editor sent no column geometry for this route and reported no visible document; {effect}. \
             The tmux columns follow the editor again once it publishes its split layout (Remote Dev: {remedy})."
        ),
        RouteLayoutEvidence::FrontendMeasurementNotReceived => format!(
            "the route deferred to the JetBrains Client's measured layout (layout_authority=frontend_measured), \
             but the controller has not received that measurement yet (for example right after a controller \
             or Client restart); {effect}. The tmux columns follow once the Client's measured columns reach the \
             controller (`[rd-layout] forwarded measured columns` in the backend idea.log). If they never do, \
             read the Remote Dev geometry hop in runbooks/coder-workspace.md. Only a build-mismatch sign means \
             the Client runs a different build: a lasting `snapshot_refused(INVALID_SCHEMA)` in the route's \
             `frontend_layout=[...]`, a backend `[split-surface] snapshot refused ... status=DELIVERY_FAILED ... \
             diagnostic=frontend_authority_absent_or_generation_mismatched` line, or a backend \
             `[split-surface] frontend/backend plugin mismatch` line. Then: {remedy}. Any other refusal \
             (for example a controller delivery timeout) is not fixed by reinstalling."
        ),
        RouteLayoutEvidence::ColumnOrderUnknown => format!(
            "the route named its documents but no column order, and the controller holds no editor \
             layout to keep yet (for example right after a controller restart); {effect}. A single \
             editor split sends no column order; the tmux columns follow the editor again on its next \
             layout sync. If this IDE is a Remote Dev backend whose layout never syncs: {remedy}."
        ),
    };
    format!(
        "{ROUTE_LAYOUT_NOT_SYNCED_MARKER} {relative_path} reason={} cause={}: {detail}",
        cause.reason(),
        cause.cause(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RouteOutcomeEvidence, RouteOutcomeKind, derive_route_outcome};

    const FLAT: RouteLayoutEvidence = RouteLayoutEvidence::FlatNoDocument;
    const MEASURED: RouteLayoutEvidence = RouteLayoutEvidence::FrontendMeasurementNotReceived;
    const UNKNOWN: RouteLayoutEvidence = RouteLayoutEvidence::ColumnOrderUnknown;

    fn all_causes() -> Vec<RouteLayoutNotSynced> {
        [FLAT, MEASURED, UNKNOWN]
            .into_iter()
            .flat_map(|evidence| {
                [
                    RouteLayoutNotSynced::EditorObservedNoDocument { evidence },
                    RouteLayoutNotSynced::DeferredFocusOnly { evidence },
                ]
            })
            .collect()
    }

    #[test]
    fn a_deferred_route_is_classified_before_the_substitution() {
        let deferred = ROUTE_LAYOUT_DEFERRED_FOCUS_ONLY_REASON;
        assert_eq!(
            RouteLayoutNotSynced::classify(true, false, deferred),
            Some(RouteLayoutNotSynced::DeferredFocusOnly { evidence: FLAT })
        );
        assert_eq!(
            RouteLayoutNotSynced::classify(false, false, deferred),
            Some(RouteLayoutNotSynced::DeferredFocusOnly { evidence: UNKNOWN })
        );
        assert_eq!(
            RouteLayoutNotSynced::classify(true, true, deferred),
            Some(RouteLayoutNotSynced::DeferredFocusOnly { evidence: MEASURED })
        );
        assert_eq!(
            RouteLayoutNotSynced::classify(true, false, "applied"),
            Some(RouteLayoutNotSynced::EditorObservedNoDocument { evidence: FLAT })
        );
        assert_eq!(
            RouteLayoutNotSynced::classify(true, true, "applied"),
            Some(RouteLayoutNotSynced::EditorObservedNoDocument { evidence: MEASURED })
        );
        assert_eq!(
            RouteLayoutNotSynced::classify(false, false, "applied"),
            None
        );
        assert_eq!(RouteLayoutNotSynced::classify(false, true, "applied"), None);
    }

    /// The notice rides a successful route's output. It must name the reason
    /// and never change what the editor reads the route as: a dispatched route
    /// stays a success, never queued, busy, or steering.
    #[test]
    fn the_notice_names_the_reason_and_keeps_the_route_a_success() {
        for cause in all_causes() {
            let notice = route_layout_not_synced_notice("tasks/a.md", cause);
            assert!(
                notice.starts_with(ROUTE_LAYOUT_NOT_SYNCED_MARKER),
                "{notice}"
            );
            assert!(
                notice.contains(&format!("reason={} ", cause.reason())),
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

    /// Each notice claims only what the route's own payload proved: only a
    /// route that named no document and declared no frontend authority is the
    /// flat Remote Dev evidence with the outright reinstall remedy; a route that
    /// declared the Client's measured layout proves the Client module is
    /// installed, so it is never told to reinstall it; and a route that named
    /// its documents in an unknown order (a local single split too) gets the
    /// remedy only conditionally.
    #[test]
    fn each_notice_claims_only_the_routes_own_evidence() {
        let reinstall = "install the same Agent Doc modular plugin ZIP";
        for cause in all_causes() {
            let notice = route_layout_not_synced_notice("tasks/a.md", cause);
            match cause.evidence() {
                RouteLayoutEvidence::FlatNoDocument => {
                    assert!(
                        notice.contains("reason=flat_evidence_has_no_column_geometry "),
                        "{notice}"
                    );
                    assert!(notice.contains("(Remote Dev: "), "{notice}");
                }
                RouteLayoutEvidence::FrontendMeasurementNotReceived => {
                    assert!(
                        notice.contains("reason=frontend_measurement_not_received "),
                        "{notice}"
                    );
                    // The reinstall remedy appears only behind the refusal
                    // and mismatch signs, never as the route's diagnosis.
                    let (diagnosis, conditional) = notice
                        .split_once("If they never do")
                        .expect("the conditional tail");
                    assert!(!diagnosis.contains(reinstall), "{notice}");
                    assert!(
                        conditional.contains("frontend/backend plugin mismatch")
                            && conditional.contains(reinstall),
                        "{notice}"
                    );
                    // The route token records only the refusal status, so only
                    // the INVALID_SCHEMA status is a mismatch sign by itself; a
                    // bare DELIVERY_FAILED also covers delivery timeouts.
                    assert!(
                        conditional.contains("snapshot_refused(INVALID_SCHEMA)")
                            && !conditional.contains("snapshot_refused(...)")
                            && !conditional.contains("snapshot_refused(DELIVERY_FAILED)"),
                        "{notice}"
                    );
                    assert!(
                        !notice.contains(ROUTE_LAYOUT_FLAT_EVIDENCE_REASON),
                        "{notice}"
                    );
                    assert!(!notice.contains("sent no column geometry"), "{notice}");
                }
                RouteLayoutEvidence::ColumnOrderUnknown => {
                    assert!(
                        notice.contains("reason=route_column_order_unknown "),
                        "{notice}"
                    );
                    assert!(
                        !notice.contains(ROUTE_LAYOUT_FLAT_EVIDENCE_REASON),
                        "{notice}"
                    );
                    assert!(!notice.contains("sent no column geometry"), "{notice}");
                    assert!(
                        notice.contains("If this IDE is a Remote Dev backend"),
                        "the remedy is conditional: {notice}"
                    );
                }
            }
        }
    }
}
