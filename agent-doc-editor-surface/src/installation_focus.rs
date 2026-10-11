//! Which editor installation drives a project's shared tmux surface
//! (`#idefocusowner`).
//!
//! A project has one tmux `agent-doc` window, but it can have several editor
//! *installations* publishing surfaces into it at once — IntelliJ IDEA and Rider
//! open on the same repository is the motivating case. Each installation has its
//! own split layout and its own selected document, and each one's surface graph
//! derives a structural `Sync` whenever tmux does not match *its* layout. Left
//! alone, every observation from either IDE re-projects tmux onto that IDE's
//! layout, so the window flips between the two shapes on each heartbeat.
//!
//! The rule is: only the installation that most recently **gained focus** drives
//! tmux. The others keep publishing (their facts stay current, their text still
//! converges), but their observations derive no tmux consequence until they are
//! focused again.
//!
//! "Gained focus" is an observed fact, not a guess: the JetBrains focus lane
//! (`jetbrains-focus@<installation>-pid:<pid>`, `focus_only`) publishes only while
//! that installation's project frame is the OS-active window, so a background
//! IDE never produces one — not even when the reverse tmux→editor mirror selects
//! a tab in it. Spanning layout observations, whose `preserve_focus`/selection
//! flags can originate from an unfocused IDE, never claim.
//!
//! The decision is a pure fold ([`InstallationFocus`]); the reactive plane is a
//! `Source` of that fold plus `Computed` projections over it
//! ([`InstallationFocusOwner`]). There is no polling: an ownership change is a
//! consequence of an accepted focus observation or of an installation retiring.

use std::collections::{BTreeMap, BTreeSet};

use agent_doc_state_scope::ProcessScope;
use lazily::{Computed, ThreadSafeContext};
use serde::{Deserialize, Serialize};

use crate::EditorSurface;

/// The installation that publishes `client_id`, if the client is
/// installation-scoped.
///
/// Client ids are `<lane>@<installation>-pid:<pid>` (`#idearidereditors`). The
/// surface lane and the focus lane of one IDE share the installation, so a
/// focus claim on the focus lane also authorizes that IDE's spanning layout.
/// Unscoped clients (`jetbrains-pid:`, `vscode-pid:`, a pre-`#idearidereditors`
/// plugin) return `None`: they neither claim nor yield ownership, which keeps
/// their behavior exactly what it was before this rule.
pub fn editor_installation(client_id: &str) -> Option<&str> {
    let (family, process_id) = client_id.split_once("-pid:")?;
    if process_id.is_empty() {
        return None;
    }
    let (lane, installation) = family.split_once('@')?;
    (!lane.is_empty() && !installation.is_empty()).then_some(installation)
}

/// Whether an accepted observation is evidence that its installation gained
/// focus.
///
/// Only the narrow focus lane qualifies; see the module docs for why spanning
/// observations cannot.
pub fn claims_installation_focus(surface: &EditorSurface) -> bool {
    surface.focus_only && !surface.focused.trim().is_empty()
}

/// The current focus owner and the epoch it has owned the surface under.
///
/// The epoch changes every time ownership moves to an installation (including
/// back to one that owned it before), so a surface graph can tell "I have been
/// driving all along" from "I drove once, lost ownership, and just got it back"
/// without remembering anything about the other installation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FocusOwner {
    pub installation: String,
    pub epoch: u64,
}

/// Pure ownership fold for one project root.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstallationFocus {
    /// installation -> the epoch of its most recent focus claim.
    claims: BTreeMap<String, u64>,
    last_epoch: u64,
}

impl InstallationFocus {
    /// The installation that most recently gained focus and is still live.
    pub fn owner(&self) -> Option<FocusOwner> {
        self.claims
            .iter()
            .max_by_key(|(_, epoch)| **epoch)
            .map(|(installation, epoch)| FocusOwner {
                installation: installation.clone(),
                epoch: *epoch,
            })
    }

    /// Record that `installation` gained focus. `None` when it already owns the
    /// surface: a repeated focus event from the owner is not a handoff, and
    /// re-stamping it would make every one of its surfaces re-project tmux.
    pub fn claim(&self, installation: &str) -> Option<Self> {
        if self
            .owner()
            .is_some_and(|owner| owner.installation == installation)
        {
            return None;
        }
        let mut next = self.clone();
        next.last_epoch = next.last_epoch.saturating_add(1);
        next.claims
            .insert(installation.to_string(), next.last_epoch);
        Some(next)
    }

    /// Drop installations that no longer have any live surface on this root.
    ///
    /// When the owner itself retires, ownership falls back to the most recently
    /// focused live installation, re-stamped with a fresh epoch so its surfaces
    /// re-project tmux instead of trusting a reconciliation the departed owner
    /// has since overwritten. `None` when nothing changed.
    pub fn retain_live(&self, live: &BTreeSet<String>) -> Option<Self> {
        if self
            .claims
            .keys()
            .all(|installation| live.contains(installation))
        {
            return None;
        }
        let previous_owner = self.owner();
        let mut next = self.clone();
        next.claims
            .retain(|installation, _| live.contains(installation));
        let fallback = next.owner();
        if let Some(fallback) = fallback
            && previous_owner.as_ref().map(|owner| &owner.installation)
                != Some(&fallback.installation)
        {
            next.last_epoch = next.last_epoch.saturating_add(1);
            next.claims.insert(fallback.installation, next.last_epoch);
        }
        Some(next)
    }

    /// The authority a surface published by `installation` has right now.
    pub fn authority_for(&self, installation: Option<&str>) -> SurfaceAuthority {
        surface_authority(self.owner().as_ref(), installation)
    }
}

/// Whether one surface graph may drive tmux.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SurfaceAuthority {
    /// This surface drives tmux under ownership `epoch`. Unscoped clients, and
    /// every client while no installation has claimed focus, drive under
    /// epoch `0`.
    Drives { epoch: u64 },
    /// Another installation owns the shared tmux surface; this surface's
    /// observations derive no consequence until it gains focus again.
    Deferred { owner: String },
}

impl Default for SurfaceAuthority {
    fn default() -> Self {
        Self::Drives { epoch: 0 }
    }
}

impl SurfaceAuthority {
    pub fn drives(&self) -> bool {
        matches!(self, Self::Drives { .. })
    }

    pub fn deferred_to(&self) -> Option<&str> {
        match self {
            Self::Drives { .. } => None,
            Self::Deferred { owner } => Some(owner),
        }
    }
}

fn surface_authority(owner: Option<&FocusOwner>, installation: Option<&str>) -> SurfaceAuthority {
    match (installation, owner) {
        (None, _) | (_, None) => SurfaceAuthority::default(),
        (Some(installation), Some(owner)) if owner.installation == installation => {
            SurfaceAuthority::Drives { epoch: owner.epoch }
        }
        (Some(_), Some(owner)) => SurfaceAuthority::Deferred {
            owner: owner.installation.clone(),
        },
    }
}

/// Reactive focus owner for one project root, living in the controller's
/// [`ProcessScope`] beside the surface graphs that consume it
/// (`#stategraphjoin`).
#[derive(Clone)]
pub struct InstallationFocusOwner {
    ctx: ThreadSafeContext,
    focus: lazily::Source<InstallationFocus>,
    owner: Computed<Option<FocusOwner>>,
}

impl InstallationFocusOwner {
    pub fn new_in(scope: &ProcessScope) -> Self {
        let ctx = scope.ctx().clone();
        let focus = ctx.source(InstallationFocus::default());
        let owner = ctx.computed(move |c| c.get(&focus).owner());
        Self { ctx, focus, owner }
    }

    /// Fold one accepted editor observation. Returns `true` when ownership
    /// moved to the observation's installation.
    pub fn observe(&self, client_id: &str, surface: &EditorSurface) -> bool {
        let Some(installation) = editor_installation(client_id) else {
            return false;
        };
        if !claims_installation_focus(surface) {
            return false;
        }
        match self.ctx.get(&self.focus).claim(installation) {
            Some(next) => {
                self.ctx.set(&self.focus, next);
                true
            }
            None => false,
        }
    }

    /// Publish the set of installations that still have a live surface.
    pub fn retain_live(&self, live: &BTreeSet<String>) {
        if let Some(next) = self.ctx.get(&self.focus).retain_live(live) {
            self.ctx.set(&self.focus, next);
        }
    }

    pub fn owner(&self) -> Option<FocusOwner> {
        self.ctx.get(&self.owner)
    }

    /// The authority projection for one client's surface graph.
    pub fn authority_for(&self, client_id: &str) -> Computed<SurfaceAuthority> {
        let installation = editor_installation(client_id).map(str::to_string);
        let owner = self.owner;
        self.ctx
            .computed(move |c| surface_authority(c.get(&owner).as_ref(), installation.as_deref()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDEA: &str = "jetbrains@IU.0a1b2c3d-pid:41";
    const IDEA_FOCUS: &str = "jetbrains-focus@IU.0a1b2c3d-pid:41";
    const RIDER: &str = "jetbrains@RD.9f8e7d6c-pid:42";
    const RIDER_FOCUS: &str = "jetbrains-focus@RD.9f8e7d6c-pid:42";

    fn focus(document: &str) -> EditorSurface {
        EditorSurface {
            focused: document.to_string(),
            visible: vec![document.to_string()],
            open: vec![document.to_string()],
            force_reconcile: true,
            focus_only: true,
            ..EditorSurface::default()
        }
    }

    fn layout(document: &str) -> EditorSurface {
        EditorSurface {
            focus_only: false,
            preserve_focus: false,
            ..focus(document)
        }
    }

    #[test]
    fn installation_is_shared_by_surface_and_focus_lanes_and_absent_when_unscoped() {
        assert_eq!(editor_installation(IDEA), Some("IU.0a1b2c3d"));
        assert_eq!(editor_installation(IDEA_FOCUS), Some("IU.0a1b2c3d"));
        assert_eq!(editor_installation("jetbrains-pid:40"), None);
        assert_eq!(editor_installation("vscode-pid:9"), None);
        assert_eq!(editor_installation("idea:42"), None);
        assert_eq!(editor_installation("jetbrains@-pid:1"), None);
        assert_eq!(editor_installation("@IU.x-pid:1"), None);
        assert_eq!(editor_installation("jetbrains@IU.x-pid:"), None);
    }

    #[test]
    fn only_a_focus_lane_observation_with_a_document_claims() {
        assert!(claims_installation_focus(&focus("/p/a.md")));
        assert!(!claims_installation_focus(&layout("/p/a.md")));
        assert!(!claims_installation_focus(&focus("  ")));
    }

    #[test]
    fn most_recent_claim_owns_and_repeated_owner_claims_keep_the_epoch() {
        let idea = InstallationFocus::default().claim("IU").unwrap();
        assert_eq!(
            idea.owner(),
            Some(FocusOwner {
                installation: "IU".to_string(),
                epoch: 1
            })
        );
        assert!(
            idea.claim("IU").is_none(),
            "owner re-focus is not a handoff"
        );
        let rider = idea.claim("RD").unwrap();
        assert_eq!(rider.owner().unwrap().installation, "RD");
        let back = rider.claim("IU").unwrap();
        assert_eq!(
            back.owner(),
            Some(FocusOwner {
                installation: "IU".to_string(),
                epoch: 3
            }),
            "regaining focus is a new ownership epoch",
        );
        assert_eq!(
            back.authority_for(Some("IU")),
            SurfaceAuthority::Drives { epoch: 3 }
        );
        assert_eq!(
            back.authority_for(Some("RD")),
            SurfaceAuthority::Deferred {
                owner: "IU".to_string()
            }
        );
        assert_eq!(
            back.authority_for(None),
            SurfaceAuthority::Drives { epoch: 0 }
        );
        assert_eq!(
            InstallationFocus::default().authority_for(Some("RD")),
            SurfaceAuthority::Drives { epoch: 0 },
            "before any focus claim every installation drives, as before",
        );
    }

    #[test]
    fn retiring_the_owner_falls_back_with_a_fresh_epoch() {
        let state = InstallationFocus::default()
            .claim("IU")
            .unwrap()
            .claim("RD")
            .unwrap();
        let live = BTreeSet::from(["RD".to_string(), "IU".to_string()]);
        assert!(state.retain_live(&live).is_none());

        let rider_gone = state
            .retain_live(&BTreeSet::from(["IU".to_string()]))
            .unwrap();
        assert_eq!(
            rider_gone.owner(),
            Some(FocusOwner {
                installation: "IU".to_string(),
                epoch: 3
            }),
            "IDEA owned epoch 1 before; its fallback must not reuse it",
        );

        let idea_gone = state
            .retain_live(&BTreeSet::from(["RD".to_string()]))
            .unwrap();
        assert_eq!(
            idea_gone.owner(),
            Some(FocusOwner {
                installation: "RD".to_string(),
                epoch: 2
            }),
            "retiring a non-owner leaves the owner's epoch alone",
        );
        assert_eq!(state.retain_live(&BTreeSet::new()).unwrap().owner(), None);
    }

    #[test]
    fn reactive_owner_projects_per_client_authority_without_a_recompute_call() {
        let scope = ProcessScope::new();
        let owner = InstallationFocusOwner::new_in(&scope);
        let idea_layout = owner.authority_for(IDEA);
        let rider_layout = owner.authority_for(RIDER);
        let unscoped = owner.authority_for("vscode-pid:9");
        let ctx = scope.ctx();

        assert!(ctx.get(&idea_layout).drives());
        assert!(ctx.get(&rider_layout).drives());

        assert!(
            !owner.observe(IDEA, &layout("/p/a.md")),
            "layout never claims"
        );
        assert!(owner.observe(IDEA_FOCUS, &focus("/p/a.md")));
        assert!(ctx.get(&idea_layout).drives());
        assert_eq!(ctx.get(&rider_layout).deferred_to(), Some("IU.0a1b2c3d"));

        assert!(owner.observe(RIDER_FOCUS, &focus("/p/b.md")));
        assert_eq!(ctx.get(&idea_layout).deferred_to(), Some("RD.9f8e7d6c"));
        assert!(ctx.get(&rider_layout).drives());
        assert!(!owner.observe(RIDER_FOCUS, &focus("/p/c.md")));
        assert!(ctx.get(&unscoped).drives());

        owner.retain_live(&BTreeSet::from(["IU.0a1b2c3d".to_string()]));
        assert_eq!(owner.owner().unwrap().installation, "IU.0a1b2c3d");
        assert!(ctx.get(&idea_layout).drives());
    }
}
