//! Which controller owns a shared tmux main-window layout (`#crossrootcolumnflip`).
//!
//! A superproject (`agent-loop`) and a nested project root with its own
//! `.agent-doc/` (`src/haiven-dev`) each run a project controller. When the
//! superproject's IDE shows a superproject document beside a submodule
//! document, both documents' panes live in ONE tmux `agent-doc` window, but
//! each controller kept its own desired-layout source and its own generation
//! counter. An editor route for the submodule document published its layout
//! on the submodule controller (gen 21, `[agent-doc-bugs, contracts]`) while
//! the superproject controller's editor-surface churn published gens 32-36
//! ending `[agent-doc-bugs, api.md]`. Two arbiters projecting into one window
//! means "whichever projects last wins", so a column could flip between
//! documents after an IDE restart and never converge to what the editor shows.
//!
//! The rule this module owns: a layout's main window is owned by the
//! controller of the **outermost project root that contains the publishing
//! controller and any column's document**. A nested controller whose layout
//! names a document from an enclosing root does not publish that layout; it
//! re-addresses the publication to the enclosing controller, which merges it
//! into its single generation sequence beside its own editor-surface
//! publications. Within one arbiter the later editor observation wins, so
//! post-restart churn converges to the editor's real visible set.
//!
//! Resolving each document's project root and talking to the other controller
//! are the adapter's job.

use std::path::Path;

/// Who publishes a layout into the shared main window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutOwner<'a> {
    /// This controller owns the window for this layout; publish locally.
    Local,
    /// An enclosing project root owns the window; re-address the publication.
    Delegate(&'a Path),
}

/// Decide who publishes a layout whose column documents resolve to
/// `column_roots` (`None` = the document's project root is unknown), on a
/// controller rooted at `controller_root`.
///
/// Only a **strictly enclosing** root triggers delegation, and the outermost
/// one wins, so the decision terminates on the owner (where no column root
/// encloses it). A column from a nested root (the superproject showing a
/// submodule document) stays local: the enclosing controller already owns
/// that window. A sibling root has no containment order and stays local, and
/// an unknown root is never delegated — without proof of an enclosing owner the
/// local publication is still the honest one.
pub fn main_window_layout_owner<'a>(
    controller_root: &Path,
    column_roots: impl IntoIterator<Item = Option<&'a Path>>,
) -> LayoutOwner<'a> {
    column_roots
        .into_iter()
        .flatten()
        .filter(|root| *root != controller_root && controller_root.starts_with(root))
        .min_by_key(|root| root.components().count())
        .map_or(LayoutOwner::Local, LayoutOwner::Delegate)
}

/// `#stalecontrollerhotupdate` (d): where one desired layout sits in the
/// editor's own publication order.
///
/// The JetBrains plugin publishes every editor-surface frame, to whichever
/// controller owns the frame's documents, from ONE process-wide client with one
/// monotonic `sequence` counter (`CpRouteClient.editorSurfaceSequence`;
/// observed live 2026-10-15: agent-loop received 77-78, haiven-dev 79-84,
/// agent-loop 85-91). `generation` is the client's connection generation, which
/// only moves up (re-registration above a retired endpoint). That makes
/// `(generation, sequence)` a total order across controllers for frames from
/// the same client, independent of when each controller got CPU time to process
/// or project its frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorPublicationOrder {
    pub client_id: String,
    pub generation: u64,
    pub sequence: u64,
}

impl EditorPublicationOrder {
    /// `Some(true)` when `self` was published after `other` by the same editor
    /// client, `Some(false)` when not, `None` when the two came from different
    /// clients and the editor order says nothing.
    pub fn is_newer_than(&self, other: &Self) -> Option<bool> {
        (self.client_id == other.client_id)
            .then(|| (self.generation, self.sequence) > (other.generation, other.sequence))
    }

    /// `<generation>|<sequence>|<client_id>`. A client id carrying a tab or a
    /// line break cannot be stored in a claim and yields `None` (the claim then
    /// falls back to its stamp).
    fn encode(&self) -> Option<String> {
        (!self.client_id.is_empty() && !self.client_id.contains(['\t', '\r', '\n']))
            .then(|| format!("{}|{}|{}", self.generation, self.sequence, self.client_id))
    }

    fn decode(value: &str) -> Option<Self> {
        let mut parts = value.splitn(3, '|');
        let generation = parts.next()?.parse().ok()?;
        let sequence = parts.next()?.parse().ok()?;
        let client_id = parts.next().filter(|client| !client.is_empty())?;
        Some(Self {
            client_id: client_id.to_string(),
            generation,
            sequence,
        })
    }
}

/// `#stalecontrollerhotupdate` (c)/(d): the newest publisher's claim on one tmux
/// window, stored as a tmux window user option so every controller on the
/// server reads the same fact and it disappears with the window.
///
/// [`main_window_layout_owner`] only orders publications that NAME documents of
/// an enclosing root. A nested controller whose layout names only its own
/// documents still projects locally, and the editor sends a later cross-root
/// frame only to the enclosing controller, so nothing superseded the nested
/// controller's stale generation: observed live (2026-10-15) haiven-dev's
/// generation 4 re-applied itself for 33 minutes, stashing agent-loop's pane on
/// every attempt while agent-loop's controller kept restoring it.
///
/// The rule: the most recent publication to drive a window owns it. "Most
/// recent" is the editor's own publication order when both publications carry
/// one from the same editor client ([`EditorPublicationOrder`]); otherwise it
/// is `stamp_ms`, minted once when the generation was PUBLISHED (never at its
/// first projection attempt, which a loaded machine can delay past a later
/// publication on another controller). Before each attempt a controller whose
/// generation is older than another root's claim yields: it retires that
/// generation instead of re-applying it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowLayoutClaim {
    pub project_root: std::path::PathBuf,
    pub stamp_ms: u64,
    pub generation: u64,
    pub publication: Option<EditorPublicationOrder>,
}

/// The tmux window user option that carries [`WindowLayoutClaim`].
pub const WINDOW_LAYOUT_CLAIM_OPTION: &str = "@agent_doc_layout_claim";

impl WindowLayoutClaim {
    /// `<stamp_ms>\t<generation>\t<publication or ->\t<project_root>`; the
    /// root is last so a path containing a tab still round-trips.
    pub fn encode(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}",
            self.stamp_ms,
            self.generation,
            self.publication
                .as_ref()
                .and_then(EditorPublicationOrder::encode)
                .unwrap_or_else(|| "-".to_string()),
            self.project_root.display()
        )
    }

    pub fn decode(value: &str) -> Option<Self> {
        let mut parts = value.trim_end_matches(['\r', '\n']).splitn(4, '\t');
        let stamp_ms = parts.next()?.trim().parse().ok()?;
        let generation = parts.next()?.trim().parse().ok()?;
        let publication = parts.next()?;
        let project_root = parts.next().filter(|root| !root.is_empty())?;
        Some(Self {
            project_root: std::path::PathBuf::from(project_root),
            stamp_ms,
            generation,
            publication: (publication != "-")
                .then(|| EditorPublicationOrder::decode(publication))
                .flatten(),
        })
    }
}

/// True when a controller rooted at `own_root`, whose current generation was
/// published at `own_stamp_ms` (editor order `own_publication`), must yield
/// the window to `claim`.
///
/// Only a strictly newer claim by a DIFFERENT root yields: a controller never
/// yields to itself (its own newer generation already superseded the older one
/// in its worker), and an absent or unreadable claim is never a reason to stop.
/// The editor order decides whenever it can; the stamp only when it cannot.
pub fn window_claim_requires_yield(
    own_root: &Path,
    own_stamp_ms: u64,
    own_publication: Option<&EditorPublicationOrder>,
    claim: Option<&WindowLayoutClaim>,
) -> bool {
    let Some(claim) = claim else {
        return false;
    };
    if claim.project_root == own_root {
        return false;
    }
    claim
        .publication
        .as_ref()
        .zip(own_publication)
        .and_then(|(theirs, ours)| theirs.is_newer_than(ours))
        .unwrap_or(claim.stamp_ms > own_stamp_ms)
}

/// `#stalecontrollerhotupdate` (c): true when a pane whose live owner document
/// belongs to `pane_owner_root` is FOREIGN to a controller rooted at
/// `controller_root` — owned by a root that is neither the controller's own
/// root nor nested beneath it.
///
/// A foreign pane is never stashed or moved by this controller unless its
/// document is one of the controller's own desired columns, and convergence
/// does not count it. The enclosing (superproject) controller still arranges
/// nested-root panes: it owns the shared main window per
/// [`main_window_layout_owner`]. An unknown owner root is never foreign, so a
/// transient process read cannot freeze a pane in place.
pub fn pane_owner_is_foreign(controller_root: &Path, pane_owner_root: Option<&Path>) -> bool {
    pane_owner_root.is_some_and(|owner| !owner.starts_with(controller_root))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const CLIENT: &str = "jetbrains@IU.8cfed3c3-pid:3190782";

    fn order(generation: u64, sequence: u64) -> EditorPublicationOrder {
        EditorPublicationOrder {
            client_id: CLIENT.to_string(),
            generation,
            sequence,
        }
    }

    fn claim(root: PathBuf, stamp_ms: u64, generation: u64) -> WindowLayoutClaim {
        WindowLayoutClaim {
            project_root: root,
            stamp_ms,
            generation,
            publication: None,
        }
    }

    #[test]
    fn window_claim_round_trips_and_rejects_garbage() {
        let plain = claim(submodule(), 1_792_094_220_000, 4);
        assert_eq!(
            WindowLayoutClaim::decode(&plain.encode()),
            Some(plain.clone())
        );
        let ordered = WindowLayoutClaim {
            publication: Some(order(1_454_943_840_525_410, 84)),
            ..plain.clone()
        };
        assert_eq!(WindowLayoutClaim::decode(&ordered.encode()), Some(ordered));
        // An unstorable client id degrades to the stamp, never to garbage.
        let tabbed = WindowLayoutClaim {
            publication: Some(EditorPublicationOrder {
                client_id: "a\tb".to_string(),
                generation: 1,
                sequence: 2,
            }),
            ..plain.clone()
        };
        assert_eq!(WindowLayoutClaim::decode(&tabbed.encode()), Some(plain));
        assert_eq!(WindowLayoutClaim::decode(""), None);
        assert_eq!(WindowLayoutClaim::decode("x\t1\t-\t/w"), None);
        assert_eq!(WindowLayoutClaim::decode("1\t2\t-\t"), None);
        assert_eq!(WindowLayoutClaim::decode("1\t2\t/w"), None);
    }

    /// The incident: haiven-dev published generation 4 first; agent-loop's later
    /// cross-root publication claimed the window, so haiven-dev yields and
    /// agent-loop never does.
    #[test]
    fn the_newest_publication_owns_a_shared_window() {
        let inner = submodule();
        let outer = superproject();
        let outer_claim = claim(outer.clone(), 2_000, 36);
        assert!(window_claim_requires_yield(
            &inner,
            1_000,
            None,
            Some(&outer_claim)
        ));
        let inner_claim = claim(inner.clone(), 1_000, 4);
        assert!(!window_claim_requires_yield(
            &outer,
            2_000,
            None,
            Some(&inner_claim)
        ));
        // Never yield to oneself, to an equal stamp, or to an absent claim.
        assert!(!window_claim_requires_yield(
            &outer,
            1_000,
            None,
            Some(&outer_claim)
        ));
        assert!(!window_claim_requires_yield(
            &inner,
            2_000,
            None,
            Some(&claim(outer.clone(), 2_000, 36))
        ));
        assert!(!window_claim_requires_yield(&inner, 0, None, None));
    }

    /// The tmux 2->1->2 regression under load: the editor's 1-pane frame
    /// (seq 84, haiven-dev) was published BEFORE its 2-pane frame (seq 86,
    /// agent-loop), but a busy machine let haiven-dev project last. Stamps
    /// taken at projection time would hand the window to the stale 1-pane
    /// layout; the editor's own order never does, in either direction.
    #[test]
    fn editor_publication_order_beats_projection_time_stamps() {
        let inner = submodule();
        let outer = superproject();
        let two_pane = WindowLayoutClaim {
            publication: Some(order(1_454_943_840_525_411, 86)),
            ..claim(outer.clone(), 1_000, 16)
        };
        // haiven-dev's stamp is LATER, its editor frame is OLDER: it yields.
        assert!(window_claim_requires_yield(
            &inner,
            9_000,
            Some(&order(1_454_943_840_525_410, 84)),
            Some(&two_pane)
        ));
        // A genuinely newer haiven-dev frame wins even with an older stamp.
        let one_pane_newer = WindowLayoutClaim {
            publication: Some(order(1_454_943_840_525_411, 92)),
            ..claim(inner.clone(), 9_000, 5)
        };
        assert!(window_claim_requires_yield(
            &outer,
            1_000,
            Some(&order(1_454_943_840_525_411, 86)),
            Some(&one_pane_newer)
        ));
        assert!(!window_claim_requires_yield(
            &inner,
            500,
            Some(&order(1_454_943_840_525_411, 92)),
            Some(&two_pane)
        ));
        // Different editor clients have no shared order: stamps decide.
        let other_client = EditorPublicationOrder {
            client_id: "vscode-1".to_string(),
            ..order(1, 999)
        };
        assert!(!window_claim_requires_yield(
            &inner,
            2_000,
            Some(&other_client),
            Some(&two_pane)
        ));
        assert!(window_claim_requires_yield(
            &inner,
            500,
            Some(&other_client),
            Some(&two_pane)
        ));
    }

    #[test]
    fn foreign_panes_are_those_outside_the_controller_subtree() {
        let inner = submodule();
        let outer = superproject();
        // haiven-dev may not stash agent-loop's agent-doc-bugs.md pane.
        assert!(pane_owner_is_foreign(&inner, Some(outer.as_path())));
        // agent-loop owns the main window and may arrange haiven-dev panes.
        assert!(!pane_owner_is_foreign(&outer, Some(inner.as_path())));
        assert!(!pane_owner_is_foreign(&inner, Some(inner.as_path())));
        let sibling = PathBuf::from("/w/agent-loop/src/tsift");
        assert!(pane_owner_is_foreign(&inner, Some(sibling.as_path())));
        let lookalike = PathBuf::from("/w/agent-loop/src/haiven-dev-old");
        assert!(pane_owner_is_foreign(&inner, Some(lookalike.as_path())));
        assert!(
            !pane_owner_is_foreign(&inner, None),
            "unknown is never foreign"
        );
    }

    fn superproject() -> PathBuf {
        PathBuf::from("/w/agent-loop")
    }

    fn submodule() -> PathBuf {
        PathBuf::from("/w/agent-loop/src/haiven-dev")
    }

    #[test]
    fn a_nested_controller_delegates_a_layout_naming_a_superproject_document() {
        // The observed gen-21 publication: [agent-doc-bugs (agent-loop),
        // contracts (haiven-dev)] published by the haiven-dev controller.
        let superproject = superproject();
        let submodule = submodule();
        assert_eq!(
            main_window_layout_owner(
                &submodule,
                [Some(superproject.as_path()), Some(submodule.as_path())]
            ),
            LayoutOwner::Delegate(superproject.as_path()),
        );
    }

    #[test]
    fn the_superproject_controller_owns_a_layout_naming_submodule_documents() {
        let superproject = superproject();
        let submodule = submodule();
        assert_eq!(
            main_window_layout_owner(
                &superproject,
                [Some(superproject.as_path()), Some(submodule.as_path())]
            ),
            LayoutOwner::Local,
            "delegation must terminate on the owner"
        );
    }

    #[test]
    fn a_layout_entirely_inside_the_nested_root_stays_local() {
        let submodule = submodule();
        assert_eq!(
            main_window_layout_owner(&submodule, [Some(submodule.as_path())]),
            LayoutOwner::Local,
        );
    }

    #[test]
    fn the_outermost_enclosing_root_wins() {
        let outer = PathBuf::from("/w");
        let middle = superproject();
        let inner = submodule();
        assert_eq!(
            main_window_layout_owner(&inner, [Some(middle.as_path()), Some(outer.as_path())]),
            LayoutOwner::Delegate(outer.as_path()),
        );
    }

    #[test]
    fn sibling_and_unknown_roots_never_delegate() {
        let submodule = submodule();
        let sibling = PathBuf::from("/w/agent-loop/src/tsift");
        let prefix_lookalike = PathBuf::from("/w/agent-loop/src/haiven");
        assert_eq!(
            main_window_layout_owner(
                &submodule,
                [
                    Some(sibling.as_path()),
                    Some(prefix_lookalike.as_path()),
                    None
                ]
            ),
            LayoutOwner::Local,
            "only a path-component ancestor encloses the controller"
        );
    }
}
