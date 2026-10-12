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

/// One lane's layout columns with every document whose pane lives in another
/// lane removed (`cross_lane_layout_dropped`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaneLayoutPartition {
    /// Columns (`file,file`) this lane may arrange; empty columns are removed.
    pub kept: Vec<String>,
    /// `(document, owner session)` for each document left in its own lane.
    pub dropped: Vec<(String, String)>,
}

/// Partition `columns` for the lane backed by `lane_session`.
///
/// `pane_session_of(document)` reports the session of the document's live pane
/// (`None` when it has none). A document whose pane lives in another lane of
/// `project_session` is dropped; every other document is kept, so a document
/// without a pane starts one in this lane and a foreign-session pane stays under
/// the existing multi-session policy.
pub fn partition_lane_layout_columns(
    columns: &[String],
    project_session: &str,
    lane_session: &str,
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
    }
}
