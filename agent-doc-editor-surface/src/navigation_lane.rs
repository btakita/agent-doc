//! Navigation lanes: which tmux surface an editor installation navigates
//! (`#ideriderindependentnav`).
//!
//! A *navigation lane* is the unit of tmux navigation authority for one project
//! root. Every layout-ownership decision (who may reshape tmux, whose focus a
//! reverse tmux-to-editor mirror reports) is keyed by `(project root, lane)`
//! through [`LayoutOwnerKey`], never by the project root alone.
//!
//! There are two modes:
//!
//! - [`NavigationMode::Shared`] (the default; `editor_navigation` in
//!   `.agent-doc/config.toml` opts out): every editor client of a root shares one lane, the project's
//!   configured tmux session. Several installations (IntelliJ IDEA and Rider on
//!   one repository) therefore compete for one `agent-doc` window, and the
//!   `#idefocusowner` fold ([`crate::InstallationFocus`]) decides which of them
//!   drives it: the one that most recently gained focus.
//! - [`NavigationMode::PerInstallation`]: each installation-scoped client
//!   (`<lane>@<installation>-pid:<pid>`, `#idearidereditors`) navigates its own
//!   lane, backed by its own tmux session. Two IDEs no longer compete, so each
//!   lane has exactly one installation and the focus-owner fold for that lane
//!   always lets it drive. Unscoped clients (`vscode-pid:`, a legacy
//!   `jetbrains-pid:`) stay on the shared lane in both modes.
//!
//! The isolation unit of a lane is a tmux **session**, not a window: tmux keeps
//! one current window per session for every attached client, so two IDE
//! terminals attached to one session cannot look at different windows, and a
//! pane belongs to exactly one session, which is the boundary the multi-session
//! policy (`ghmultitmux`) already refuses to move panes across. Each lane
//! session keeps the canonical `agent-doc` and `stash` windows.
//!
//! Everything here is pure: no IO, no tmux, no state.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::installation_focus::editor_installation;

/// How a project's editor installations share tmux navigation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NavigationMode {
    /// One lane per project root; the most recently focused installation
    /// drives it (`#idefocusowner`).
    #[default]
    Shared,
    /// One lane (tmux session) per editor installation.
    PerInstallation,
}

impl NavigationMode {
    /// Parse the configuration spelling (`shared` / `per_installation`, with
    /// `-` accepted for `_`). Unknown values are `None` so a caller can report
    /// them instead of silently picking a mode.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "shared" => Some(Self::Shared),
            "per_installation" => Some(Self::PerInstallation),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shared => "shared",
            Self::PerInstallation => "per_installation",
        }
    }
}

/// The tmux navigation lane an editor client publishes into.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "installation", rename_all = "snake_case")]
pub enum NavigationLane {
    /// The project's configured tmux session, shared by every client.
    #[default]
    Shared,
    /// One editor installation's own tmux session.
    Installation(String),
}

impl NavigationLane {
    /// The lane `client_id` navigates under `mode`.
    ///
    /// Only an installation-scoped client in per-installation mode gets its own
    /// lane; everything else (shared mode, unscoped clients, an empty id) is the
    /// shared lane, which is exactly the pre-lane behavior.
    pub fn for_client(mode: NavigationMode, client_id: &str) -> Self {
        match (mode, editor_installation(client_id)) {
            (NavigationMode::PerInstallation, Some(installation)) => {
                Self::Installation(installation.to_string())
            }
            _ => Self::Shared,
        }
    }

    pub fn installation(&self) -> Option<&str> {
        match self {
            Self::Shared => None,
            Self::Installation(installation) => Some(installation),
        }
    }

    pub fn is_shared(&self) -> bool {
        matches!(self, Self::Shared)
    }

    /// A stable diagnostic label: `shared`, or the installation id.
    pub fn label(&self) -> &str {
        self.installation().unwrap_or("shared")
    }

    /// The state-plane channel that carries this lane's copy of the per-lane
    /// projection published on `base_channel`.
    ///
    /// The shared lane is `base_channel` itself, so a shared-mode subscriber
    /// and every unscoped client (VS Code) read exactly the pre-lane channel.
    /// An installation lane is `<base_channel>/lane/<installation>`: a
    /// distinct channel, so one lane's frames never reach another lane's
    /// subscriber and never displace the shared channel's retained snapshot.
    pub fn state_channel(&self, base_channel: &str) -> String {
        match self {
            Self::Shared => base_channel.to_string(),
            Self::Installation(installation) => format!("{base_channel}/lane/{installation}"),
        }
    }

    /// The tmux session backing this lane, derived from the project's
    /// configured session.
    ///
    /// The shared lane is the configured session itself. An installation lane
    /// is `<project session>-<installation>` with every character tmux would
    /// rewrite or misparse in a target (`.`, `:`, whitespace, ...) mapped to
    /// `-`, so `agent-loop` + `IU.0a1b2c3d` is `agent-loop-IU-0a1b2c3d`.
    pub fn tmux_session_name(&self, project_session: &str) -> String {
        match self {
            Self::Shared => project_session.to_string(),
            Self::Installation(installation) => {
                format!(
                    "{project_session}-{}",
                    tmux_session_name_component(installation)
                )
            }
        }
    }
}

/// `value` restricted to characters that are safe in a tmux session name and
/// unambiguous in a tmux target (`[A-Za-z0-9_-]`).
pub fn tmux_session_name_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

/// Whether `session` is an installation lane session derived from
/// `project_session` (`<project session>-<installation component>`), i.e. a
/// name [`NavigationLane::tmux_session_name`] can produce for some installation.
///
/// This is a naming relation, not proof that agent-doc created the session; it
/// is used only to *refuse* (never to adopt or reap) a pane that lives in a
/// sibling lane, so a false positive fails closed.
pub fn is_lane_session_of(project_session: &str, session: &str) -> bool {
    let project_session = project_session.trim();
    if project_session.is_empty() {
        return false;
    }
    session
        .strip_prefix(project_session)
        .and_then(|rest| rest.strip_prefix('-'))
        .is_some_and(|component| {
            !component.is_empty()
                && component
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
        })
}

/// Where a document's existing pane lives relative to one navigation lane
/// (`#ideriderindependentnav` P2b).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LanePaneMembership {
    /// The document has no live pane; a new one starts in the lane's session.
    Unplaced,
    /// The pane already lives in this lane's session.
    InLane,
    /// The pane lives in another lane of the same project (the shared project
    /// session or a sibling installation lane session). Sync for this lane must
    /// drop it and focus must refuse it: a pane never changes lane implicitly.
    OtherLane { owner_session: String },
    /// The pane lives in a session that is not a lane of this project; the
    /// existing session policy (`tmux_sessions`, GH #17) governs it.
    Foreign { session: String },
}

impl LanePaneMembership {
    /// Classify `pane_session` (the session the document's pane lives in, if
    /// any) against `lane_session`, a lane of `project_session`.
    pub fn classify(project_session: &str, lane_session: &str, pane_session: Option<&str>) -> Self {
        let Some(pane_session) = pane_session.map(str::trim).filter(|s| !s.is_empty()) else {
            return Self::Unplaced;
        };
        if pane_session == lane_session {
            Self::InLane
        } else if pane_session == project_session
            || is_lane_session_of(project_session, pane_session)
        {
            Self::OtherLane {
                owner_session: pane_session.to_string(),
            }
        } else {
            Self::Foreign {
                session: pane_session.to_string(),
            }
        }
    }

    pub fn owned_by_other_lane(&self) -> bool {
        matches!(self, Self::OtherLane { .. })
    }
}

/// The diagnostic owner-lane label of a session of `project_session`:
/// `shared` for the project session, else the sanitized installation component.
pub fn lane_label_for_session<'a>(project_session: &str, session: &'a str) -> &'a str {
    if session == project_session {
        return "shared";
    }
    session
        .strip_prefix(project_session)
        .and_then(|rest| rest.strip_prefix('-'))
        .filter(|component| !component.is_empty())
        .unwrap_or(session)
}

/// How a lane's layout publication treats a document whose pane lives in
/// another lane (`#ideriderindependentnav` P4).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum LaneClaim {
    /// A passive publication (editor heartbeat, focus escalation, route,
    /// resync, a republish): the pane stays in the lane that owns it and the
    /// document is dropped from this lane (`cross_lane_layout_dropped`).
    #[default]
    Passive,
    /// An explicit operator request from this lane (`Sync Tmux Layout`, the
    /// layout sync that follows `Claim for Tmux Pane`): the pane is handed
    /// over to this lane (`navigation_lane_handoff`).
    Explicit,
}

impl LaneClaim {
    /// The claim a layout invocation's `caller_kind` makes. Only `manual`, the
    /// kind an operator's own sync or claim action publishes, is explicit;
    /// every other kind (`automatic`, `editor_route`, `resync`, `projection`,
    /// `lane_handoff`, ...) is passive, so no background publication can ever
    /// move a pane between lanes.
    pub fn for_caller_kind(caller_kind: &str) -> Self {
        if caller_kind.trim() == "manual" {
            Self::Explicit
        } else {
            Self::Passive
        }
    }

    pub fn is_explicit(self) -> bool {
        matches!(self, Self::Explicit)
    }
}

/// The `caller_kind` of a lane republished after a handoff took one of its
/// panes. It is passive ([`LaneClaim::for_caller_kind`]), so the republish can
/// only drop the document, never take the pane back.
pub const LANE_HANDOFF_CALLER_KIND: &str = "lane_handoff";

/// The `caller_kind` of an installation lane's recycle-settle republish when
/// its retained layout was an explicit claim (`#ideriderindependentnav` P6).
/// It is passive ([`LaneClaim::for_caller_kind`]), so a background republish
/// never hands a pane over from another lane.
pub const RECYCLE_SETTLED_CALLER_KIND: &str = "recycle_settled";

/// One lane's layout columns with every document whose pane lives in another
/// lane removed (`cross_lane_layout_dropped`), or, for an explicit claim,
/// marked for handoff into this lane.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaneLayoutPartition {
    /// Columns (`file,file`) this lane may arrange; empty columns are removed.
    pub kept: Vec<String>,
    /// `(document, owner session)` for each document left in its own lane.
    pub dropped: Vec<(String, String)>,
    /// `(document, owner session)` for each document whose pane an explicit
    /// claim hands over to this lane (P4). These documents stay in `kept`.
    pub handoffs: Vec<(String, String)>,
}

/// The session a sync of the shared navigation lane arranges, given the session
/// the sync body resolved (`#ridernavp6switchback`).
///
/// After `editor_navigation` switches from `per_installation` back to `shared`,
/// installation lane sessions agent-doc created stay alive until `agent-doc gc`
/// folds their panes back. Until then the sync body can resolve one of them as
/// its target: the focused document's pane, or the tmux client's current
/// session, may still be there. In shared mode no editor navigates a lane, so
/// the shared sync arranges the project session instead, and a document whose
/// pane is still in the lane is adopted into it (same pane, joined across
/// sessions), never started a second time. Every other case keeps `resolved`:
/// per-installation mode (lanes are live), a session that only carries a lane's
/// name (`lane_marked` false: agent-doc did not create it, so the pre-lane
/// rules apply), and a dead project session (nothing to arrange instead).
pub fn shared_sync_target_session<'a>(
    mode: NavigationMode,
    project_session: &'a str,
    resolved: &'a str,
    lane_marked: bool,
    project_session_alive: bool,
) -> &'a str {
    if mode == NavigationMode::Shared
        && lane_marked
        && project_session_alive
        && resolved != project_session
        && is_lane_session_of(project_session, resolved)
    {
        project_session
    } else {
        resolved
    }
}

/// The lane among `lanes` whose session is `session`, for a project whose
/// configured session is `project_session`. Used to republish the lane a
/// handoff took a pane from.
pub fn lane_owning_session<'a>(
    lanes: impl IntoIterator<Item = &'a NavigationLane>,
    project_session: &str,
    session: &str,
) -> Option<NavigationLane> {
    lanes
        .into_iter()
        .find(|lane| lane.tmux_session_name(project_session) == session)
        .cloned()
}

/// Partition `columns` for the lane backed by `lane_session` under a passive
/// claim: [`partition_lane_layout_columns_for_claim`] with
/// [`LaneClaim::Passive`].
pub fn partition_lane_layout_columns(
    columns: &[String],
    project_session: &str,
    lane_session: &str,
    pane_session_of: impl FnMut(&str) -> Option<String>,
) -> LaneLayoutPartition {
    partition_lane_layout_columns_for_claim(
        columns,
        project_session,
        lane_session,
        LaneClaim::Passive,
        pane_session_of,
    )
}

/// Partition `columns` for the lane backed by `lane_session`.
///
/// `pane_session_of(document)` reports the session of the document's live pane
/// (`None` when it has none). A document whose pane lives in another lane of
/// `project_session` is dropped under a passive claim and kept plus listed in
/// `handoffs` under an explicit one; every other document is kept, so a
/// document without a pane starts one in this lane and a foreign-session pane
/// stays under the existing multi-session policy (never handed over).
pub fn partition_lane_layout_columns_for_claim(
    columns: &[String],
    project_session: &str,
    lane_session: &str,
    claim: LaneClaim,
    mut pane_session_of: impl FnMut(&str) -> Option<String>,
) -> LaneLayoutPartition {
    let mut partition = LaneLayoutPartition::default();
    for column in columns {
        let mut kept = Vec::new();
        for file in column.split(',').map(str::trim).filter(|f| !f.is_empty()) {
            let pane_session = pane_session_of(file);
            match LanePaneMembership::classify(
                project_session,
                lane_session,
                pane_session.as_deref(),
            ) {
                LanePaneMembership::OtherLane { owner_session } if claim.is_explicit() => {
                    partition.handoffs.push((file.to_string(), owner_session));
                    kept.push(file);
                }
                LanePaneMembership::OtherLane { owner_session } => {
                    partition.dropped.push((file.to_string(), owner_session));
                }
                _ => kept.push(file),
            }
        }
        if !kept.is_empty() {
            partition.kept.push(kept.join(","));
        }
    }
    partition
}

/// The navigation lane a route, start, or `agent-doc claim` acts for
/// (`#ideriderindependentnav` P6): the lane of `requester` (an editor surface
/// client id passed as `--client-id` or in an editor route payload), or the
/// shared lane when the request names no client.
pub fn requester_navigation_lane(mode: NavigationMode, requester: Option<&str>) -> NavigationLane {
    requester
        .map(str::trim)
        .filter(|client| !client.is_empty())
        .map_or(NavigationLane::Shared, |client| {
            NavigationLane::for_client(mode, client)
        })
}

/// Where a route, start, or `agent-doc claim` on behalf of a requester puts a
/// document's pane (`#ideriderindependentnav` P6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequesterPanePlacement {
    /// The document already has a live pane in `session`; the request uses it
    /// where it lives. A request never moves a pane between lanes: only an
    /// explicit layout claim (P4) hands one over.
    Existing { session: String },
    /// The document has no live pane; a new one starts in `session`, the
    /// requester's lane session (the project session for the shared lane).
    Start {
        session: String,
        lane: NavigationLane,
    },
}

impl RequesterPanePlacement {
    /// The session the request's pane lives in, or starts in.
    pub fn session(&self) -> &str {
        match self {
            Self::Existing { session } | Self::Start { session, .. } => session,
        }
    }
}

/// Place a route, start, or claim request from `requester` for a document
/// whose live pane (if any) lives in `existing_pane_session`.
///
/// In `shared` mode, and for a request without a client or from an unscoped
/// client, a new pane starts in `project_session`, exactly as before lanes. In
/// `per_installation` mode an installation-scoped requester's new pane starts
/// in its own lane session. An existing pane is always used in place.
pub fn requester_pane_placement(
    mode: NavigationMode,
    project_session: &str,
    requester: Option<&str>,
    existing_pane_session: Option<&str>,
) -> RequesterPanePlacement {
    if let Some(session) = existing_pane_session
        .map(str::trim)
        .filter(|session| !session.is_empty())
    {
        return RequesterPanePlacement::Existing {
            session: session.to_string(),
        };
    }
    let lane = requester_navigation_lane(mode, requester);
    RequesterPanePlacement::Start {
        session: lane.tmux_session_name(project_session),
        lane,
    }
}

/// What an editor route (Run Agent Doc, `editor_route`) from a requester's
/// lane does with its layout (`#ridernavp6routecontended`).
///
/// A route is a passive lane claim ([`LaneClaim::for_caller_kind`] of
/// `editor_route`), so the requester's lane drops a document whose pane lives
/// in another lane from every frame it projects (`cross_lane_layout_dropped`).
/// Publishing the route's layout for such a document therefore waits on a
/// frame that can never show it. Instead the route publishes nothing and
/// dispatches into the document's existing pane where it lives: that pane is
/// the document's single live owner, so no second harness starts, and the
/// pane is not moved (only an explicit claim hands it over, P4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequesterRouteLayout {
    /// Publish the route's layout into the requester's lane and await the
    /// frame that shows the document: every `shared`-mode route, a document
    /// without a live pane, a pane already in the requester's lane, and a pane
    /// in a session that is no lane of this project.
    Publish,
    /// The document's pane lives in `owner_session`, another lane of this
    /// project: publish no layout, dispatch into that pane, and tell the
    /// requester where it is ([`ROUTE_OWNED_BY_OTHER_LANE_REASON`]).
    DispatchInOtherLane { owner_session: String },
}

/// The reason an editor route reports when it dispatched into a pane that
/// lives in another navigation lane (`#ridernavp6routecontended`). It is the
/// same reason a lane's editor focus refusal reports for that pane.
pub const ROUTE_OWNED_BY_OTHER_LANE_REASON: &str = "owned_by_other_lane";

/// The prefix of the notice line an editor route appends to its output when
/// it dispatched into another lane's pane ([`route_owned_by_other_lane_notice`]).
/// Editor plugins match it to tell the operator where the document runs.
pub const ROUTE_OWNED_BY_OTHER_LANE_NOTICE_PREFIX: &str = "[agent-doc] owned_by_other_lane:";

/// Decide what an editor route from `lane` does with its layout for a document
/// whose live pane (if any) lives in `pane_session`.
///
/// Only `per_installation` mode has lanes; `shared` mode always publishes, so
/// its route is the pre-lane one byte for byte.
pub fn requester_route_layout(
    mode: NavigationMode,
    project_session: &str,
    lane: &NavigationLane,
    pane_session: Option<&str>,
) -> RequesterRouteLayout {
    if mode != NavigationMode::PerInstallation || project_session.trim().is_empty() {
        return RequesterRouteLayout::Publish;
    }
    match LanePaneMembership::classify(
        project_session,
        &lane.tmux_session_name(project_session),
        pane_session,
    ) {
        LanePaneMembership::OtherLane { owner_session } => {
            RequesterRouteLayout::DispatchInOtherLane { owner_session }
        }
        _ => RequesterRouteLayout::Publish,
    }
}

/// The operator-facing notice of a route that dispatched `document` into
/// `pane`, which lives in `owner_session`, another lane of `project_session`.
pub fn route_owned_by_other_lane_notice(
    document: &str,
    pane: &str,
    project_session: &str,
    owner_session: &str,
) -> String {
    format!(
        "{ROUTE_OWNED_BY_OTHER_LANE_NOTICE_PREFIX} {document} runs in pane {pane} of navigation lane {} (tmux session {owner_session}); the route dispatched it there and left this IDE's tmux layout unchanged. Use Claim for Tmux Pane to move the pane into this IDE's lane.",
        lane_label_for_session(project_session, owner_session),
    )
}

/// One pane of an installation lane session, as the lane reconcile sees it
/// (`#ideriderindependentnav` P5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanePaneObservation {
    pub pane: String,
    /// The pane is a document pane agent-doc owns (a session-registry or
    /// actor binding of this project names it).
    pub document_owned: bool,
    /// The pane's foreground command is not a known bare shell. An unknown
    /// command counts as busy (false-alive over false-dead). A shell pane
    /// agent-doc itself created in the lane is never busy while no client has
    /// ever attached to the session (`#lanegcflake`): its startup commands are
    /// not the operator's work.
    pub busy: bool,
}

/// One tmux session whose name is an installation lane of the project's
/// session, as the lane reconcile sees it (`#ideriderindependentnav` P5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneSessionObservation {
    pub session: String,
    /// The session carries `@agent_doc_navigation_lane` = this project root,
    /// i.e. agent-doc created it. An unmarked session is never touched
    /// (`#bare-foreign-session-guard`).
    pub marked: bool,
    /// A live surface still uses the lane: a tmux client (an IDE terminal) is
    /// attached to it, or the controller reports its installation live.
    pub live_surface: bool,
    pub panes: Vec<LanePaneObservation>,
}

/// Why the lane reconcile keeps a lane session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LaneRetainReason {
    /// Agent-doc did not create it (no lane mark for this root).
    NotMarked,
    /// An IDE terminal or a live editor installation still uses it.
    LiveSurface,
    /// A document pane agent-doc owns still lives in it (per-installation
    /// mode keeps it; shared mode folds it first).
    OwnedPanes,
    /// A pane runs something other than a bare shell and no document owns it:
    /// the operator's work, which agent-doc never moves or kills.
    BusyUnownedPane,
    /// Shared mode would fold its document panes back, but the project
    /// session to fold them into is not alive.
    ProjectSessionMissing,
}

impl LaneRetainReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotMarked => "not_marked",
            Self::LiveSurface => "live_surface",
            Self::OwnedPanes => "owned_panes",
            Self::BusyUnownedPane => "busy_unowned_pane",
            Self::ProjectSessionMissing => "project_session_missing",
        }
    }
}

/// One step of the lane reconcile (`#ideriderindependentnav` P5).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LaneReconcileAction {
    /// Disable migration: move a document pane out of an installation lane
    /// session into the project session's `stash` window, from where the
    /// shared lane's sync places it. The pane id (and its actor binding) is
    /// unchanged; nothing is restarted.
    FoldPane { session: String, pane: String },
    /// Close an empty lane session agent-doc created: no document pane, no
    /// busy pane, no live surface. Only bare shells (its initial pane, its
    /// stash shell, a handoff placeholder) die with it, and a lane that is
    /// needed again is provisioned again on its next driving layout.
    Close { session: String },
    /// Keep the session.
    Retain {
        session: String,
        reason: LaneRetainReason,
    },
}

/// Plan the lane reconcile for one project (`agent-doc gc`).
///
/// `lanes` are the sessions whose names are installation lanes of
/// `project_session` ([`is_lane_session_of`]); any other session is ignored,
/// so the project session itself and unrelated sessions are never planned.
///
/// - Every mode: an unmarked session is retained (`NotMarked`) and nothing in
///   it is moved; a session in use by a live surface is never closed.
/// - [`NavigationMode::PerInstallation`] (enable migration): no pane is ever
///   moved. Panes that lived in the shared session before the switch stay
///   there; an installation lane acquires panes only through its own layouts
///   and explicit claims. A lane is closed only when it is empty and unused.
/// - [`NavigationMode::Shared`] (disable migration): every lane is retired.
///   Each document pane folds back into the project session (when that session
///   is alive), then the lane closes unless a busy unowned pane or a live
///   surface keeps it. No pane is duplicated or lost: a fold moves the same
///   pane, and a close only ever takes bare shells.
pub fn plan_navigation_lane_reconcile(
    mode: NavigationMode,
    project_session: &str,
    project_session_alive: bool,
    lanes: &[LaneSessionObservation],
) -> Vec<LaneReconcileAction> {
    let mut actions = Vec::new();
    for lane in lanes {
        if !is_lane_session_of(project_session, &lane.session) {
            continue;
        }
        let retain = |reason| LaneReconcileAction::Retain {
            session: lane.session.clone(),
            reason,
        };
        if !lane.marked {
            actions.push(retain(LaneRetainReason::NotMarked));
            continue;
        }
        let owned = lane
            .panes
            .iter()
            .filter(|pane| pane.document_owned)
            .collect::<Vec<_>>();
        let busy_unowned = lane
            .panes
            .iter()
            .any(|pane| pane.busy && !pane.document_owned);
        match mode {
            NavigationMode::PerInstallation => {
                if lane.live_surface {
                    actions.push(retain(LaneRetainReason::LiveSurface));
                } else if !owned.is_empty() {
                    actions.push(retain(LaneRetainReason::OwnedPanes));
                } else if busy_unowned {
                    actions.push(retain(LaneRetainReason::BusyUnownedPane));
                } else {
                    actions.push(LaneReconcileAction::Close {
                        session: lane.session.clone(),
                    });
                }
            }
            NavigationMode::Shared => {
                if !owned.is_empty() && !project_session_alive {
                    actions.push(retain(LaneRetainReason::ProjectSessionMissing));
                    continue;
                }
                for pane in owned {
                    actions.push(LaneReconcileAction::FoldPane {
                        session: lane.session.clone(),
                        pane: pane.pane.clone(),
                    });
                }
                if busy_unowned {
                    actions.push(retain(LaneRetainReason::BusyUnownedPane));
                } else if lane.live_surface {
                    actions.push(retain(LaneRetainReason::LiveSurface));
                } else {
                    actions.push(LaneReconcileAction::Close {
                        session: lane.session.clone(),
                    });
                }
            }
        }
    }
    actions
}

/// Which installation lanes a lane reconcile treats as in use by a live editor
/// installation (`#ideriderindependentnav` P5 follow-up).
///
/// An attached tmux client and a busy pane keep a lane in every case; this is
/// only the editor-liveness half of [`LaneSessionObservation::live_surface`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LaneLiveness {
    /// `agent-doc gc`: exactly these installations are live, every other
    /// installation is not. The CLI knows no editor, so it passes an empty set
    /// and an attached terminal is its only liveness proof.
    Live(std::collections::BTreeSet<String>),
    /// The project controller: only these installations are proven gone (it
    /// saw them live and then saw every one of their surfaces retire); every
    /// other installation's lane counts as live. A controller that never saw
    /// an installation (it quit before this controller started, or has not
    /// reconnected yet) cannot tell it is gone, so its lane is left to `gc`.
    RetiredOnly(std::collections::BTreeSet<String>),
}

impl LaneLiveness {
    /// Whether `session` is the lane of an installation this evidence counts
    /// as live, for lanes of `project_session`.
    pub fn lane_session_live(&self, project_session: &str, session: &str) -> bool {
        let lane_of = |installation: &String| {
            NavigationLane::Installation(installation.clone()).tmux_session_name(project_session)
                == session
        };
        match self {
            Self::Live(live) => live.iter().any(lane_of),
            Self::RetiredOnly(retired) => !retired.iter().any(lane_of),
        }
    }
}

/// The controller-side request to run the lane reconcile
/// (`#ideriderindependentnav` P5 follow-up, `#ridernavp6gc`).
///
/// The controller derives it from the navigation mode it runs under and the
/// editor installations it has seen live; its `Effect` runs the reconcile each
/// time the derived request changes, so neither an IDE that quits nor a
/// `per_installation` → `shared` switch waits for `agent-doc gc`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LaneReconcileTrigger {
    pub mode: NavigationMode,
    /// Installations this controller saw live whose every surface has since
    /// retired (a relaunched installation leaves this set again).
    pub retired: std::collections::BTreeSet<String>,
}

impl LaneReconcileTrigger {
    /// The liveness evidence the reconcile runs on.
    ///
    /// In `per_installation` mode only the retired installations' lanes may
    /// close. In `shared` mode no installation navigates a lane any more, so
    /// no installation keeps one: each lane folds its document panes back and
    /// closes unless an attached terminal or a busy pane keeps it, exactly as
    /// `agent-doc gc` does.
    pub fn liveness(&self) -> LaneLiveness {
        match self.mode {
            NavigationMode::PerInstallation => LaneLiveness::RetiredOnly(self.retired.clone()),
            NavigationMode::Shared => LaneLiveness::Live(Default::default()),
        }
    }

    /// A short label for logs: `mode=<mode> retired=<a,b>`.
    pub fn describe(&self) -> String {
        format!(
            "mode={} retired={}",
            self.mode.as_str(),
            self.retired
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(",")
        )
    }
}

/// Derive the controller's lane reconcile request, or `None` when there is
/// nothing a reconcile could do yet.
///
/// - `mode` is the mode the controller runs under. It is read once per
///   controller start, so a mode switch reaches here as a controller start
///   with the new mode.
/// - `seen` are the editor installations this controller saw live, `live` the
///   ones live now (an installation is live while any of its surfaces is).
///
/// `shared` always requests a reconcile: the first one is the disable
/// migration (fold lane panes back, close the lanes), and a later retirement
/// re-requests it so a lane an attached terminal kept is closed once that
/// IDE goes away. `per_installation` requests one only once an installation
/// it saw has retired: enabling moves no pane, and a newly live installation
/// never makes a lane closable.
pub fn derive_lane_reconcile_trigger(
    mode: NavigationMode,
    seen: &std::collections::BTreeSet<String>,
    live: &std::collections::BTreeSet<String>,
) -> Option<LaneReconcileTrigger> {
    let retired = seen
        .difference(live)
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    match mode {
        NavigationMode::Shared => Some(LaneReconcileTrigger { mode, retired }),
        NavigationMode::PerInstallation if retired.is_empty() => None,
        NavigationMode::PerInstallation => Some(LaneReconcileTrigger { mode, retired }),
    }
}

/// The key every layout-ownership decision is made under: one project root's
/// one navigation lane. In shared mode every client of a root maps to the same
/// key, so keying by it is identical to the historical per-root keying.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LayoutOwnerKey {
    pub root: PathBuf,
    pub lane: NavigationLane,
}

impl LayoutOwnerKey {
    pub fn new(root: impl Into<PathBuf>, lane: NavigationLane) -> Self {
        Self {
            root: root.into(),
            lane,
        }
    }

    /// The key `client_id` publishes under for `root` in `mode`.
    pub fn for_client(root: impl Into<PathBuf>, mode: NavigationMode, client_id: &str) -> Self {
        Self::new(root, NavigationLane::for_client(mode, client_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn shared_sync_retargets_only_a_marked_lane_of_the_project_in_shared_mode() {
        let lane = "proj-IU-0a1b2c3d";
        assert_eq!(
            shared_sync_target_session(NavigationMode::Shared, "proj", lane, true, true),
            "proj",
            "a stale marked lane is replaced by the project session"
        );
        assert_eq!(
            shared_sync_target_session(NavigationMode::PerInstallation, "proj", lane, true, true),
            lane,
            "per-installation lanes stay live targets"
        );
        assert_eq!(
            shared_sync_target_session(NavigationMode::Shared, "proj", lane, false, true),
            lane,
            "an unmarked lane-named session keeps the pre-lane rules"
        );
        assert_eq!(
            shared_sync_target_session(NavigationMode::Shared, "proj", lane, true, false),
            lane,
            "a dead project session has nothing to arrange instead"
        );
        for other in ["proj", "scratch", "proj-", "other-IU-1"] {
            assert_eq!(
                shared_sync_target_session(NavigationMode::Shared, "proj", other, true, true),
                other,
                "{other} is not a lane of proj"
            );
        }
    }

    const IDEA: &str = "jetbrains@IU.0a1b2c3d-pid:41";
    const IDEA_FOCUS: &str = "jetbrains-focus@IU.0a1b2c3d-pid:41";
    const RIDER: &str = "jetbrains@RD.9f8e7d6c-pid:42";

    #[test]
    fn shared_mode_puts_every_client_on_the_shared_lane() {
        for client in [
            IDEA,
            IDEA_FOCUS,
            RIDER,
            "vscode-pid:9",
            "jetbrains-pid:7",
            "",
        ] {
            assert_eq!(
                NavigationLane::for_client(NavigationMode::Shared, client),
                NavigationLane::Shared,
                "{client}"
            );
        }
    }

    #[test]
    fn lane_state_channel_keeps_the_shared_channel_and_separates_installations() {
        const BASE: &str = "agent-doc/pane-layout/status/v1";
        for mode in [NavigationMode::Shared, NavigationMode::PerInstallation] {
            assert_eq!(
                NavigationLane::for_client(mode, "vscode-pid:9").state_channel(BASE),
                BASE,
                "an unscoped client reads the pre-lane channel in {mode:?}"
            );
        }
        for client in [IDEA, RIDER] {
            assert_eq!(
                NavigationLane::for_client(NavigationMode::Shared, client).state_channel(BASE),
                BASE,
                "shared mode keeps every client on the pre-lane channel"
            );
        }
        let mode = NavigationMode::PerInstallation;
        let idea = NavigationLane::for_client(mode, IDEA).state_channel(BASE);
        let idea_focus = NavigationLane::for_client(mode, IDEA_FOCUS).state_channel(BASE);
        let rider = NavigationLane::for_client(mode, RIDER).state_channel(BASE);
        assert_eq!(idea, "agent-doc/pane-layout/status/v1/lane/IU.0a1b2c3d");
        assert_eq!(idea_focus, idea, "one IDE's clients share its lane channel");
        assert_ne!(rider, idea);
        assert_ne!(rider, BASE);
    }

    #[test]
    fn per_installation_mode_gives_each_installation_its_own_lane() {
        let mode = NavigationMode::PerInstallation;
        let idea = NavigationLane::for_client(mode, IDEA);
        assert_eq!(
            idea,
            NavigationLane::Installation("IU.0a1b2c3d".to_string())
        );
        assert_eq!(
            NavigationLane::for_client(mode, IDEA_FOCUS),
            idea,
            "the surface and focus lanes of one IDE share its navigation lane"
        );
        assert_ne!(NavigationLane::for_client(mode, RIDER), idea);
        assert_eq!(
            NavigationLane::for_client(mode, "vscode-pid:9"),
            NavigationLane::Shared,
            "an unscoped client stays on the shared lane"
        );
        assert_eq!(
            NavigationLane::for_client(mode, "jetbrains-pid:7"),
            NavigationLane::Shared
        );
    }

    #[test]
    fn lane_sessions_derive_from_the_project_session() {
        assert_eq!(
            NavigationLane::Shared.tmux_session_name("agent-loop"),
            "agent-loop"
        );
        assert_eq!(
            NavigationLane::Installation("IU.0a1b2c3d".to_string()).tmux_session_name("agent-loop"),
            "agent-loop-IU-0a1b2c3d"
        );
        assert_eq!(
            NavigationLane::Installation("RD.9f:8 e".to_string()).tmux_session_name("0"),
            "0-RD-9f-8-e"
        );
        assert_eq!(NavigationLane::Shared.label(), "shared");
        assert_eq!(
            NavigationLane::Installation("RD.9f8e7d6c".to_string()).label(),
            "RD.9f8e7d6c"
        );
    }

    #[test]
    fn mode_parses_config_spellings_and_rejects_unknown_values() {
        assert_eq!(
            NavigationMode::parse("shared"),
            Some(NavigationMode::Shared)
        );
        assert_eq!(
            NavigationMode::parse(" Per-Installation "),
            Some(NavigationMode::PerInstallation)
        );
        assert_eq!(
            NavigationMode::parse("per_installation"),
            Some(NavigationMode::PerInstallation)
        );
        assert_eq!(NavigationMode::parse("per_window"), None);
        assert_eq!(NavigationMode::default(), NavigationMode::Shared);
        for mode in [NavigationMode::Shared, NavigationMode::PerInstallation] {
            assert_eq!(NavigationMode::parse(mode.as_str()), Some(mode));
        }
    }

    #[test]
    fn layout_owner_key_collapses_to_the_root_in_shared_mode() {
        let shared = |client| LayoutOwnerKey::for_client("/p", NavigationMode::Shared, client);
        assert_eq!(shared(IDEA), shared(RIDER));
        let per =
            |client| LayoutOwnerKey::for_client("/p", NavigationMode::PerInstallation, client);
        assert_ne!(per(IDEA), per(RIDER));
        assert_eq!(per(IDEA), per(IDEA_FOCUS));
        assert_ne!(
            per(IDEA),
            LayoutOwnerKey::for_client("/q", NavigationMode::PerInstallation, IDEA)
        );
    }

    #[test]
    fn lane_sessions_are_recognized_only_as_derived_from_the_project_session() {
        assert!(is_lane_session_of("agent-loop", "agent-loop-IU-0a1b2c3d"));
        assert!(!is_lane_session_of("agent-loop", "agent-loop"));
        assert!(!is_lane_session_of("agent-loop", "agent-loop-"));
        assert!(!is_lane_session_of("agent-loop", "agent-loopX-IU"));
        assert!(!is_lane_session_of("agent-loop", "other-IU-0a1b2c3d"));
        assert!(!is_lane_session_of("agent-loop", "agent-loop-IU.0a"));
        assert!(!is_lane_session_of("", "-IU"));
        assert_eq!(lane_label_for_session("p", "p"), "shared");
        assert_eq!(lane_label_for_session("p", "p-RD-9f8e7d6c"), "RD-9f8e7d6c");
        assert_eq!(lane_label_for_session("p", "elsewhere"), "elsewhere");
    }

    #[test]
    fn pane_membership_separates_own_lane_other_lanes_and_foreign_sessions() {
        let classify = |pane| LanePaneMembership::classify("p", "p-IU-0a1b2c3d", pane);
        assert_eq!(classify(None), LanePaneMembership::Unplaced);
        assert_eq!(classify(Some("  ")), LanePaneMembership::Unplaced);
        assert_eq!(classify(Some("p-IU-0a1b2c3d")), LanePaneMembership::InLane);
        assert!(classify(Some("p")).owned_by_other_lane());
        assert!(classify(Some("p-RD-9f8e7d6c")).owned_by_other_lane());
        assert_eq!(
            classify(Some("research")),
            LanePaneMembership::Foreign {
                session: "research".to_string()
            }
        );
        // The shared lane treats installation lanes as other lanes too.
        assert!(
            LanePaneMembership::classify("p", "p", Some("p-IU-0a1b2c3d")).owned_by_other_lane()
        );
        assert_eq!(
            LanePaneMembership::classify("p", "p", Some("p")),
            LanePaneMembership::InLane
        );
    }

    #[test]
    fn lane_partition_drops_only_documents_whose_pane_lives_in_another_lane() {
        let columns = vec![
            "/r/a.md".to_string(),
            "/r/b.md, /r/c.md".to_string(),
            "/r/d.md".to_string(),
        ];
        let sessions = BTreeMap::from([
            ("/r/a.md", "p-IU-0a"),
            ("/r/b.md", "p"),
            ("/r/d.md", "p-RD-9f"),
        ]);
        let partition = partition_lane_layout_columns(&columns, "p", "p-IU-0a", |file| {
            sessions.get(file).map(|session| session.to_string())
        });
        assert_eq!(partition.kept, vec!["/r/a.md", "/r/c.md"]);
        assert_eq!(
            partition.dropped,
            vec![
                ("/r/b.md".to_string(), "p".to_string()),
                ("/r/d.md".to_string(), "p-RD-9f".to_string()),
            ]
        );
    }

    #[test]
    fn only_a_manual_caller_makes_an_explicit_lane_claim() {
        assert_eq!(LaneClaim::for_caller_kind("manual"), LaneClaim::Explicit);
        assert_eq!(LaneClaim::for_caller_kind(" manual "), LaneClaim::Explicit);
        for passive in [
            "automatic",
            "editor_route",
            "resync",
            "projection",
            "",
            LANE_HANDOFF_CALLER_KIND,
            RECYCLE_SETTLED_CALLER_KIND,
        ] {
            assert_eq!(
                LaneClaim::for_caller_kind(passive),
                LaneClaim::Passive,
                "{passive:?}"
            );
        }
    }

    #[test]
    fn an_explicit_claim_hands_other_lanes_panes_over_and_keeps_them() {
        let columns = vec![
            "/r/a.md".to_string(),
            "/r/b.md, /r/c.md".to_string(),
            "/r/d.md".to_string(),
            "/r/e.md".to_string(),
        ];
        let sessions = BTreeMap::from([
            ("/r/a.md", "p-IU-0a"),
            ("/r/b.md", "p"),
            ("/r/d.md", "p-RD-9f"),
            ("/r/e.md", "research"),
        ]);
        let partition = partition_lane_layout_columns_for_claim(
            &columns,
            "p",
            "p-IU-0a",
            LaneClaim::Explicit,
            |file| sessions.get(file).map(|session| session.to_string()),
        );
        assert_eq!(
            partition.kept,
            vec!["/r/a.md", "/r/b.md,/r/c.md", "/r/d.md", "/r/e.md"]
        );
        assert!(partition.dropped.is_empty());
        assert_eq!(
            partition.handoffs,
            vec![
                ("/r/b.md".to_string(), "p".to_string()),
                ("/r/d.md".to_string(), "p-RD-9f".to_string()),
            ],
            "only panes in another lane of this project are handed over; a foreign session's pane is not"
        );
        let passive = partition_lane_layout_columns(&columns, "p", "p-IU-0a", |file| {
            sessions.get(file).map(|session| session.to_string())
        });
        assert!(passive.handoffs.is_empty());
        assert_eq!(passive.dropped.len(), 2);
    }

    #[test]
    fn the_lane_owning_a_session_is_found_among_known_lanes() {
        let idea = NavigationLane::Installation("IU.0a".to_string());
        let rider = NavigationLane::Installation("RD.9f".to_string());
        let lanes = [NavigationLane::Shared, idea.clone(), rider.clone()];
        assert_eq!(
            lane_owning_session(&lanes, "p", "p"),
            Some(NavigationLane::Shared)
        );
        assert_eq!(lane_owning_session(&lanes, "p", "p-RD-9f"), Some(rider));
        assert_eq!(lane_owning_session(&lanes, "p", "p-XX-00"), None);
        assert_eq!(lane_owning_session(&lanes[..1], "p", "p-IU-0a"), None);
    }

    #[test]
    fn a_requester_starts_a_new_pane_in_its_own_lane_only_in_per_installation_mode() {
        let start = |mode, requester| requester_pane_placement(mode, "p", requester, None);
        assert_eq!(
            start(NavigationMode::PerInstallation, Some(IDEA)),
            RequesterPanePlacement::Start {
                session: "p-IU-0a1b2c3d".to_string(),
                lane: NavigationLane::Installation("IU.0a1b2c3d".to_string()),
            }
        );
        assert_eq!(
            start(NavigationMode::PerInstallation, Some(RIDER)).session(),
            "p-RD-9f8e7d6c"
        );
        for (mode, requester) in [
            (NavigationMode::Shared, Some(IDEA)),
            (NavigationMode::Shared, None),
            (NavigationMode::PerInstallation, None),
            (NavigationMode::PerInstallation, Some("vscode-pid:9")),
            (NavigationMode::PerInstallation, Some("  ")),
        ] {
            assert_eq!(
                start(mode, requester),
                RequesterPanePlacement::Start {
                    session: "p".to_string(),
                    lane: NavigationLane::Shared,
                },
                "{mode:?} {requester:?} keeps the pre-lane project session"
            );
        }
    }

    #[test]
    fn a_requester_uses_an_existing_pane_where_it_lives() {
        for existing in ["p", "p-RD-9f8e7d6c", "elsewhere"] {
            assert_eq!(
                requester_pane_placement(
                    NavigationMode::PerInstallation,
                    "p",
                    Some(IDEA),
                    Some(existing)
                ),
                RequesterPanePlacement::Existing {
                    session: existing.to_string()
                },
                "a request never moves a pane between lanes"
            );
        }
        assert_eq!(
            requester_pane_placement(NavigationMode::PerInstallation, "p", Some(IDEA), Some(" "))
                .session(),
            "p-IU-0a1b2c3d",
            "a blank pane session is no pane"
        );
    }

    #[test]
    fn a_route_from_another_lane_dispatches_in_place_without_publishing() {
        let idea = NavigationLane::for_client(NavigationMode::PerInstallation, IDEA);
        let rider = NavigationLane::for_client(NavigationMode::PerInstallation, RIDER);
        let route = |lane: &NavigationLane, pane: Option<&str>| {
            requester_route_layout(NavigationMode::PerInstallation, "p", lane, pane)
        };
        // Rider routes a document whose pane lives in IDEA's lane, or in the
        // shared project session: dispatch there, publish nothing.
        for owner in ["p-IU-0a1b2c3d", "p"] {
            assert_eq!(
                route(&rider, Some(owner)),
                RequesterRouteLayout::DispatchInOtherLane {
                    owner_session: owner.to_string()
                }
            );
        }
        // The shared lane (VS Code) in per-installation mode: a pane in an
        // installation lane is another lane's too.
        assert_eq!(
            route(&NavigationLane::Shared, Some("p-IU-0a1b2c3d")),
            RequesterRouteLayout::DispatchInOtherLane {
                owner_session: "p-IU-0a1b2c3d".to_string()
            }
        );
        // No pane, a pane in the requester's own lane, or a foreign session:
        // the route publishes and awaits its layout as before.
        for pane in [None, Some(" "), Some("p-IU-0a1b2c3d"), Some("elsewhere")] {
            assert_eq!(
                route(&idea, pane),
                RequesterRouteLayout::Publish,
                "{pane:?}"
            );
        }
        // Shared mode, or no project session to derive lanes from, is the
        // pre-lane route for every pane.
        for (mode, session) in [
            (NavigationMode::Shared, "p"),
            (NavigationMode::PerInstallation, " "),
        ] {
            for pane in [Some("p"), Some("p-IU-0a1b2c3d"), Some("p-RD-9f8e7d6c")] {
                assert_eq!(
                    requester_route_layout(mode, session, &rider, pane),
                    RequesterRouteLayout::Publish
                );
            }
        }
    }

    /// The JetBrains plugin matches the notice by this prefix.
    #[test]
    fn the_jetbrains_plugin_matches_the_route_notice_prefix() {
        let source = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/TerminalUtil.kt"
        ))
        .unwrap();
        assert!(
            source.contains(&format!(
                "ROUTE_OWNED_BY_OTHER_LANE_NOTICE_PREFIX = \"{ROUTE_OWNED_BY_OTHER_LANE_NOTICE_PREFIX}\""
            )),
            "TerminalUtil.kt must match the route notice prefix"
        );
    }

    #[test]
    fn the_route_notice_names_the_owning_lane_and_the_claim() {
        let notice = route_owned_by_other_lane_notice("plan.md", "%7", "p", "p-IU-0a1b2c3d");
        assert!(
            notice.starts_with(ROUTE_OWNED_BY_OTHER_LANE_NOTICE_PREFIX),
            "{notice}"
        );
        assert!(
            notice.contains("pane %7 of navigation lane IU-0a1b2c3d"),
            "{notice}"
        );
        assert!(notice.contains("tmux session p-IU-0a1b2c3d"), "{notice}");
        assert!(notice.contains("Claim for Tmux Pane"), "{notice}");
        assert!(!notice.contains('\n'));
        assert!(
            route_owned_by_other_lane_notice("plan.md", "%7", "p", "p")
                .contains("navigation lane shared")
        );
    }

    proptest! {
        #[test]
        fn requester_placement_never_moves_and_starts_only_in_the_requester_lane(
            product in "[A-Z]{2}",
            hash in "[0-9a-f]{8}",
            per_installation in any::<bool>(),
            existing in proptest::option::of("[a-z]{1,4}(-[A-Z]{2}-[0-9a-f]{8})?"),
        ) {
            let mode = if per_installation {
                NavigationMode::PerInstallation
            } else {
                NavigationMode::Shared
            };
            let client = format!("jetbrains@{product}.{hash}-pid:1");
            let placement = requester_pane_placement(mode, "p", Some(&client), existing.as_deref());
            match (&existing, &placement) {
                (Some(existing), RequesterPanePlacement::Existing { session }) => {
                    prop_assert_eq!(existing, session);
                }
                (None, RequesterPanePlacement::Start { session, lane }) => {
                    prop_assert_eq!(session, &NavigationLane::for_client(mode, &client).tmux_session_name("p"));
                    prop_assert_eq!(lane.is_shared(), !per_installation);
                }
                _ => prop_assert!(false, "{existing:?} -> {placement:?}"),
            }
        }
    }

    proptest! {
        #[test]
        fn a_route_never_publishes_a_frame_its_lane_must_drop(
            requester in proptest::option::of("[A-Z]{2}\\.[0-9a-f]{8}"),
            per_installation in any::<bool>(),
            pane in proptest::option::of("p(-[A-Z]{2}-[0-9a-f]{8})?|elsewhere"),
        ) {
            let mode = if per_installation {
                NavigationMode::PerInstallation
            } else {
                NavigationMode::Shared
            };
            let lane = requester
                .as_deref()
                .map_or(NavigationLane::Shared, |installation| {
                    NavigationLane::for_client(mode, &format!("jetbrains@{installation}-pid:1"))
                });
            let lane_session = lane.tmux_session_name("p");
            let decision = requester_route_layout(mode, "p", &lane, pane.as_deref());
            // A published route frame keeps the document under the passive
            // partition its lane applies, so the route's wait can converge.
            let partition = partition_lane_layout_columns(
                &["plan.md".to_string()],
                "p",
                &lane_session,
                |_| pane.clone(),
            );
            match &decision {
                RequesterRouteLayout::Publish => {
                    prop_assert!(!per_installation || partition.dropped.is_empty());
                }
                RequesterRouteLayout::DispatchInOtherLane { owner_session } => {
                    prop_assert!(per_installation);
                    prop_assert_eq!(Some(owner_session), pane.as_ref());
                    prop_assert_eq!(partition.dropped.len(), 1);
                }
            }
        }
    }

    fn lane(
        session: &str,
        marked: bool,
        live: bool,
        panes: &[(&str, bool, bool)],
    ) -> LaneSessionObservation {
        LaneSessionObservation {
            session: session.to_string(),
            marked,
            live_surface: live,
            panes: panes
                .iter()
                .map(|(pane, document_owned, busy)| LanePaneObservation {
                    pane: pane.to_string(),
                    document_owned: *document_owned,
                    busy: *busy,
                })
                .collect(),
        }
    }

    #[test]
    fn enabling_lanes_never_moves_a_pane_and_closes_only_empty_unused_lanes() {
        let lanes = [
            lane(
                "p-IU-0a",
                true,
                false,
                &[("%1", false, false), ("%2", false, false)],
            ),
            lane("p-RD-9f", true, false, &[("%3", true, true)]),
            lane("p-CL-11", true, true, &[("%4", false, false)]),
            lane("p-PY-22", true, false, &[("%5", false, true)]),
            lane("p-XX-33", false, false, &[]),
            lane("p", true, false, &[("%9", true, true)]),
            lane("other", true, false, &[]),
        ];
        let plan =
            plan_navigation_lane_reconcile(NavigationMode::PerInstallation, "p", true, &lanes);
        assert_eq!(
            plan,
            vec![
                LaneReconcileAction::Close {
                    session: "p-IU-0a".to_string()
                },
                LaneReconcileAction::Retain {
                    session: "p-RD-9f".to_string(),
                    reason: LaneRetainReason::OwnedPanes
                },
                LaneReconcileAction::Retain {
                    session: "p-CL-11".to_string(),
                    reason: LaneRetainReason::LiveSurface
                },
                LaneReconcileAction::Retain {
                    session: "p-PY-22".to_string(),
                    reason: LaneRetainReason::BusyUnownedPane
                },
                LaneReconcileAction::Retain {
                    session: "p-XX-33".to_string(),
                    reason: LaneRetainReason::NotMarked
                },
            ],
            "the project session and unrelated sessions are never planned"
        );
    }

    #[test]
    fn disabling_lanes_folds_document_panes_back_then_closes_the_lane() {
        let lanes = [
            lane(
                "p-IU-0a",
                true,
                false,
                &[
                    ("%1", true, true),
                    ("%2", false, false),
                    ("%3", true, false),
                ],
            ),
            lane("p-RD-9f", true, true, &[("%4", true, true)]),
            lane(
                "p-CL-11",
                true,
                false,
                &[("%5", true, true), ("%6", false, true)],
            ),
            lane("p-XX-33", false, false, &[("%7", true, true)]),
        ];
        let plan = plan_navigation_lane_reconcile(NavigationMode::Shared, "p", true, &lanes);
        let fold = |session: &str, pane: &str| LaneReconcileAction::FoldPane {
            session: session.to_string(),
            pane: pane.to_string(),
        };
        assert_eq!(
            plan,
            vec![
                fold("p-IU-0a", "%1"),
                fold("p-IU-0a", "%3"),
                LaneReconcileAction::Close {
                    session: "p-IU-0a".to_string()
                },
                fold("p-RD-9f", "%4"),
                LaneReconcileAction::Retain {
                    session: "p-RD-9f".to_string(),
                    reason: LaneRetainReason::LiveSurface
                },
                fold("p-CL-11", "%5"),
                LaneReconcileAction::Retain {
                    session: "p-CL-11".to_string(),
                    reason: LaneRetainReason::BusyUnownedPane
                },
                LaneReconcileAction::Retain {
                    session: "p-XX-33".to_string(),
                    reason: LaneRetainReason::NotMarked
                },
            ]
        );
        let missing =
            plan_navigation_lane_reconcile(NavigationMode::Shared, "p", false, &lanes[..1]);
        assert_eq!(
            missing,
            vec![LaneReconcileAction::Retain {
                session: "p-IU-0a".to_string(),
                reason: LaneRetainReason::ProjectSessionMissing
            }],
            "with no project session to fold into, nothing moves and nothing closes"
        );
        let empty = plan_navigation_lane_reconcile(
            NavigationMode::Shared,
            "p",
            false,
            &[lane("p-IU-0a", true, false, &[("%1", false, false)])],
        );
        assert_eq!(
            empty,
            vec![LaneReconcileAction::Close {
                session: "p-IU-0a".to_string()
            }]
        );
    }

    fn installations(names: &[&str]) -> std::collections::BTreeSet<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    #[test]
    fn per_installation_reconciles_only_after_a_seen_installation_retires() {
        let mode = NavigationMode::PerInstallation;
        // Controller start: nothing seen, nothing to prove gone.
        assert_eq!(
            derive_lane_reconcile_trigger(mode, &installations(&[]), &installations(&[])),
            None
        );
        // An installation joins: a new live installation never makes a lane
        // closable.
        let seen = installations(&["IU.0a", "RD.9f"]);
        assert_eq!(derive_lane_reconcile_trigger(mode, &seen, &seen), None);
        // Rider quits.
        let trigger =
            derive_lane_reconcile_trigger(mode, &seen, &installations(&["IU.0a"])).unwrap();
        assert_eq!(trigger.retired, installations(&["RD.9f"]));
        assert_eq!(
            trigger.liveness(),
            LaneLiveness::RetiredOnly(installations(&["RD.9f"]))
        );
        assert_eq!(trigger.describe(), "mode=per_installation retired=RD.9f");
        // Rider relaunches: nothing is proven gone again.
        assert_eq!(derive_lane_reconcile_trigger(mode, &seen, &seen), None);
    }

    #[test]
    fn shared_mode_always_reconciles_and_counts_no_installation_live() {
        let mode = NavigationMode::Shared;
        // Controller start under `shared`: the disable migration runs at once.
        let start =
            derive_lane_reconcile_trigger(mode, &installations(&[]), &installations(&[])).unwrap();
        assert_eq!(start.liveness(), LaneLiveness::Live(installations(&[])));
        // A retirement changes the request, so the reconcile runs again (a
        // lane an attached terminal kept can close now).
        let seen = installations(&["IU.0a"]);
        let retired = derive_lane_reconcile_trigger(mode, &seen, &installations(&[])).unwrap();
        assert_ne!(retired, start);
        assert_eq!(retired.liveness(), LaneLiveness::Live(installations(&[])));
    }

    #[test]
    fn lane_liveness_maps_installations_to_their_lane_sessions() {
        let live = LaneLiveness::Live(installations(&["IU.0a"]));
        assert!(live.lane_session_live("p", "p-IU-0a"));
        assert!(!live.lane_session_live("p", "p-RD-9f"));
        assert!(!LaneLiveness::Live(installations(&[])).lane_session_live("p", "p-IU-0a"));
        let retired = LaneLiveness::RetiredOnly(installations(&["RD.9f"]));
        assert!(!retired.lane_session_live("p", "p-RD-9f"));
        // Never seen retire: an unknown installation's lane is left alone.
        assert!(retired.lane_session_live("p", "p-IU-0a"));
        assert!(retired.lane_session_live("p", "p-CL-11"));
    }

    use std::collections::BTreeMap;

    proptest! {
        #[test]
        fn lane_partition_never_keeps_a_pane_from_another_lane(
            placements in proptest::collection::vec(0usize..4, 1..6),
        ) {
            let sessions = [None, Some("p-IU-0a"), Some("p"), Some("p-RD-9f")];
            let columns = (0..placements.len())
                .map(|index| format!("/r/{index}.md"))
                .collect::<Vec<_>>();
            let partition = partition_lane_layout_columns(&columns, "p", "p-IU-0a", |file| {
                let index: usize = file
                    .trim_start_matches("/r/")
                    .trim_end_matches(".md")
                    .parse()
                    .unwrap();
                sessions[placements[index]].map(str::to_string)
            });
            prop_assert_eq!(partition.kept.len() + partition.dropped.len(), columns.len());
            for kept in &partition.kept {
                let index: usize = kept
                    .trim_start_matches("/r/")
                    .trim_end_matches(".md")
                    .parse()
                    .unwrap();
                prop_assert!(placements[index] <= 1);
            }
        }

        #[test]
        fn an_explicit_claim_never_loses_a_document_and_hands_over_only_other_lanes(
            placements in proptest::collection::vec(0usize..5, 1..6),
        ) {
            let sessions = [None, Some("p-IU-0a"), Some("p"), Some("p-RD-9f"), Some("research")];
            let columns = (0..placements.len())
                .map(|index| format!("/r/{index}.md"))
                .collect::<Vec<_>>();
            let index_of = |file: &str| -> usize {
                file.trim_start_matches("/r/").trim_end_matches(".md").parse().unwrap()
            };
            let partition = partition_lane_layout_columns_for_claim(
                &columns,
                "p",
                "p-IU-0a",
                LaneClaim::Explicit,
                |file| sessions[placements[index_of(file)]].map(str::to_string),
            );
            prop_assert!(partition.dropped.is_empty());
            prop_assert_eq!(partition.kept.len(), columns.len());
            for (document, owner) in &partition.handoffs {
                let placement = placements[index_of(document)];
                prop_assert!(placement == 2 || placement == 3);
                prop_assert_eq!(Some(owner.as_str()), sessions[placement]);
            }
            let other_lane = placements.iter().filter(|p| **p == 2 || **p == 3).count();
            prop_assert_eq!(partition.handoffs.len(), other_lane);
        }

        #[test]
        fn the_lane_reconcile_never_loses_a_document_pane_or_touches_foreign_sessions(
            shape in proptest::collection::vec(
                (any::<bool>(), any::<bool>(), proptest::collection::vec((any::<bool>(), any::<bool>()), 0..4)),
                0..4,
            ),
            shared in any::<bool>(),
            project_alive in any::<bool>(),
        ) {
            let lanes = shape
                .iter()
                .enumerate()
                .map(|(index, (marked, live, panes))| LaneSessionObservation {
                    session: format!("p-L{index}"),
                    marked: *marked,
                    live_surface: *live,
                    panes: panes
                        .iter()
                        .enumerate()
                        .map(|(pane, (document_owned, busy))| LanePaneObservation {
                            pane: format!("%{index}{pane}"),
                            document_owned: *document_owned,
                            busy: *busy,
                        })
                        .collect(),
                })
                .collect::<Vec<_>>();
            let mode = if shared { NavigationMode::Shared } else { NavigationMode::PerInstallation };
            let plan = plan_navigation_lane_reconcile(mode, "p", project_alive, &lanes);
            for lane in &lanes {
                let folded = plan
                    .iter()
                    .filter_map(|action| match action {
                        LaneReconcileAction::FoldPane { session, pane } if session == &lane.session => Some(pane.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                let closed = plan.iter().any(|action| matches!(action, LaneReconcileAction::Close { session } if session == &lane.session));
                if !lane.marked {
                    prop_assert!(folded.is_empty() && !closed);
                }
                if mode == NavigationMode::PerInstallation {
                    prop_assert!(folded.is_empty(), "enable never moves a pane");
                }
                if closed {
                    prop_assert!(!lane.live_surface);
                    for pane in &lane.panes {
                        prop_assert!(!pane.busy || pane.document_owned);
                        prop_assert!(!pane.document_owned || folded.contains(&pane.pane));
                    }
                }
                if !folded.is_empty() {
                    prop_assert!(project_alive);
                }
            }
        }

        #[test]
        fn installation_lane_sessions_are_tmux_safe_and_distinct_from_the_shared_lane(
            project in "[A-Za-z0-9_-]{1,12}",
            installation in "\\PC{1,24}",
        ) {
            let lane = NavigationLane::Installation(installation);
            let session = lane.tmux_session_name(&project);
            let prefix = format!("{project}-");
            prop_assert!(session.starts_with(&prefix));
            prop_assert!(session
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'));
            prop_assert!(is_lane_session_of(&project, &session));
            prop_assert_ne!(session, NavigationLane::Shared.tmux_session_name(&project));
        }

        #[test]
        fn a_controller_trigger_never_counts_a_live_installation_gone(
            seen in prop::collection::btree_set("[A-Z]{2}\\.[0-9a-f]{2}", 0..5),
            live in prop::collection::btree_set("[A-Z]{2}\\.[0-9a-f]{2}", 0..5),
        ) {
            if let Some(trigger) =
                derive_lane_reconcile_trigger(NavigationMode::PerInstallation, &seen, &live)
            {
                prop_assert!(!trigger.retired.is_empty());
                for installation in &live {
                    let session =
                        NavigationLane::Installation(installation.clone()).tmux_session_name("p");
                    prop_assert!(trigger.liveness().lane_session_live("p", &session));
                }
                for installation in &trigger.retired {
                    prop_assert!(seen.contains(installation) && !live.contains(installation));
                }
            }
        }
    }
}
