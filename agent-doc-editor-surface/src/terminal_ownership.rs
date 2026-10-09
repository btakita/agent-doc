//! Exclusive editor-surface ownership of the IDE-hosted agent terminal.
//!
//! A JetBrains project may have several editor frames after “Show Tab in New
//! Window”, but the IDE-hosted agent terminal is one presentation. This fold is
//! the single policy owner for deciding which frame may mount it. Adapters only
//! report per-frame facts and apply the returned decision.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SurfaceRevision {
    pub generation: u64,
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceOwnershipObservation {
    #[serde(default)]
    pub source_id: String,
    pub surface_id: String,
    pub generation: u64,
    pub sequence: u64,
    #[serde(default)]
    pub authoritative_focus: bool,
    #[serde(default)]
    pub focused: String,
    #[serde(default)]
    pub visible: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalMount {
    pub surface_id: String,
    pub document: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SurfaceTerminalDecision {
    Stale,
    #[default]
    Idle,
    AlreadyMounted {
        surface_id: String,
        document: String,
    },
    Mount {
        surface_id: String,
        document: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        previous: Option<TerminalMount>,
    },
    Exclude {
        surface_id: String,
        document: String,
        owner_surface_id: String,
    },
    Stash {
        previous: TerminalMount,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SurfaceSnapshot {
    surface_id: String,
    order: u64,
    authoritative_focus: bool,
    revision: SurfaceRevision,
    focused: String,
    visible: BTreeSet<String>,
}

/// Pure, deterministic ownership state. The controller ProcessScope retains
/// this value and advances it exactly once for each accepted surface fact.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SurfaceTerminalOwnership {
    surfaces: BTreeMap<(String, String), SurfaceSnapshot>,
    owners: BTreeMap<String, String>,
    mounted: Option<TerminalMount>,
    next_order: u64,
}

impl SurfaceTerminalOwnership {
    pub fn mounted(&self) -> Option<&TerminalMount> {
        self.mounted.as_ref()
    }

    pub fn owner_of(&self, document: &str) -> Option<&str> {
        self.owners.get(document).map(String::as_str)
    }

    pub fn observe(&mut self, observation: SurfaceOwnershipObservation) -> SurfaceTerminalDecision {
        if observation.surface_id.trim().is_empty() {
            return SurfaceTerminalDecision::Stale;
        }
        let revision = SurfaceRevision {
            generation: observation.generation,
            sequence: observation.sequence,
        };
        let key = (
            observation.source_id.clone(),
            observation.surface_id.clone(),
        );
        if self
            .surfaces
            .get(&key)
            .is_some_and(|current| revision <= current.revision)
        {
            return SurfaceTerminalDecision::Stale;
        }
        let incoming_surface_id = observation.surface_id.clone();
        let incoming_focused = observation.focused.clone();
        self.next_order = self.next_order.saturating_add(1);
        self.surfaces.insert(
            key,
            SurfaceSnapshot {
                surface_id: observation.surface_id,
                order: self.next_order,
                authoritative_focus: observation.authoritative_focus,
                revision,
                focused: observation.focused,
                visible: observation.visible.into_iter().collect(),
            },
        );
        self.recompute(incoming_surface_id, incoming_focused)
    }

    pub fn retire(
        &mut self,
        source_id: &str,
        surface_id: &str,
        generation: u64,
    ) -> SurfaceTerminalDecision {
        let key = (source_id.to_string(), surface_id.to_string());
        let Some(current) = self.surfaces.get(&key) else {
            return SurfaceTerminalDecision::Stale;
        };
        if generation != current.revision.generation {
            return SurfaceTerminalDecision::Stale;
        }
        self.surfaces.remove(&key);
        self.recompute(surface_id.to_string(), String::new())
    }

    fn recompute(
        &mut self,
        incoming_surface_id: String,
        incoming_focused: String,
    ) -> SurfaceTerminalDecision {
        let all_documents = self
            .surfaces
            .values()
            .flat_map(|surface| surface.visible.iter().cloned())
            .collect::<BTreeSet<_>>();
        self.owners.retain(|document, owner| {
            all_documents.contains(document)
                && self.surfaces.values().any(|surface| {
                    surface.surface_id.as_str() == owner.as_str()
                        && surface.visible.contains(document)
                })
        });

        for document in all_documents {
            let focused_owner = self
                .surfaces
                .iter()
                .filter(|(_, surface)| surface.focused == document)
                .max_by(|(left_key, left), (right_key, right)| {
                    left.authoritative_focus
                        .cmp(&right.authoritative_focus)
                        .then_with(|| {
                            left.order
                                .cmp(&right.order)
                                .then_with(|| left_key.cmp(right_key))
                        })
                })
                .map(|(_, surface)| surface.surface_id.clone());
            let retained_owner = self
                .owners
                .get(&document)
                .filter(|surface_id| {
                    self.surfaces.values().any(|surface| {
                        surface.surface_id.as_str() == surface_id.as_str()
                            && surface.visible.contains(&document)
                    })
                })
                .cloned();
            let newest_visible = self
                .surfaces
                .iter()
                .filter(|(_, surface)| surface.visible.contains(&document))
                .max_by(|(left_key, left), (right_key, right)| {
                    left.order
                        .cmp(&right.order)
                        .then_with(|| left_key.cmp(right_key))
                })
                .map(|(_, surface)| surface.surface_id.clone());
            if let Some(owner) = focused_owner.or(retained_owner).or(newest_visible) {
                self.owners.insert(document, owner);
            }
        }

        let desired = self
            .surfaces
            .iter()
            .filter(|(_, surface)| {
                !surface.focused.is_empty() && surface.visible.contains(&surface.focused)
            })
            .max_by(|(left_key, left), (right_key, right)| {
                left.authoritative_focus
                    .cmp(&right.authoritative_focus)
                    .then_with(|| {
                        left.order
                            .cmp(&right.order)
                            .then_with(|| left_key.cmp(right_key))
                    })
            })
            .map(|(_, surface)| TerminalMount {
                surface_id: surface.surface_id.clone(),
                document: surface.focused.clone(),
            })
            .or_else(|| {
                self.mounted.as_ref().and_then(|mounted| {
                    self.owner_of(&mounted.document).map(|owner| TerminalMount {
                        surface_id: owner.to_string(),
                        document: mounted.document.clone(),
                    })
                })
            });

        match (self.mounted.clone(), desired) {
            (Some(previous), Some(next)) if previous == next => {
                self.mounted = Some(next.clone());
                if incoming_surface_id != next.surface_id && incoming_focused == next.document {
                    SurfaceTerminalDecision::Exclude {
                        surface_id: incoming_surface_id,
                        document: next.document,
                        owner_surface_id: next.surface_id,
                    }
                } else {
                    SurfaceTerminalDecision::AlreadyMounted {
                        surface_id: next.surface_id,
                        document: next.document,
                    }
                }
            }
            (previous, Some(next)) => {
                self.owners
                    .insert(next.document.clone(), next.surface_id.clone());
                self.mounted = Some(next.clone());
                SurfaceTerminalDecision::Mount {
                    surface_id: next.surface_id,
                    document: next.document,
                    previous,
                }
            }
            (Some(previous), None) => {
                self.mounted = None;
                SurfaceTerminalDecision::Stash { previous }
            }
            (None, None) => SurfaceTerminalDecision::Idle,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(
        surface_id: &str,
        generation: u64,
        sequence: u64,
        focused: &str,
        visible: &[&str],
    ) -> SurfaceOwnershipObservation {
        SurfaceOwnershipObservation {
            source_id: "test".to_string(),
            surface_id: surface_id.to_string(),
            generation,
            sequence,
            authoritative_focus: true,
            focused: focused.to_string(),
            visible: visible.iter().map(|value| (*value).to_string()).collect(),
        }
    }

    #[test]
    fn move_to_detached_surface_atomically_transfers_and_excludes_old_surface() {
        let mut ownership = SurfaceTerminalOwnership::default();
        assert!(matches!(
            ownership.observe(observation("main", 1, 1, "/p/a.md", &["/p/a.md"])),
            SurfaceTerminalDecision::Mount { surface_id, previous: None, .. }
                if surface_id == "main"
        ));
        assert!(matches!(
            ownership.observe(observation("detached-7", 1, 2, "/p/a.md", &["/p/a.md"])),
            SurfaceTerminalDecision::Mount { surface_id, previous: Some(previous), .. }
                if surface_id == "detached-7" && previous.surface_id == "main"
        ));
        assert_eq!(ownership.owner_of("/p/a.md"), Some("detached-7"));
        assert!(matches!(
            ownership.observe(observation("main", 1, 3, "", &["/p/a.md"])),
            SurfaceTerminalDecision::AlreadyMounted { surface_id, .. }
                if surface_id == "detached-7"
        ));
    }

    #[test]
    fn two_windows_showing_same_document_keep_the_focused_owner() {
        let mut ownership = SurfaceTerminalOwnership::default();
        ownership.observe(observation("main", 2, 1, "/p/a.md", &["/p/a.md"]));
        ownership.observe(observation("detached", 2, 2, "/p/a.md", &["/p/a.md"]));
        assert_eq!(ownership.owner_of("/p/a.md"), Some("detached"));
        assert!(matches!(
            ownership.observe(observation("main", 2, 3, "", &["/p/a.md"])),
            SurfaceTerminalDecision::AlreadyMounted { surface_id, .. }
                if surface_id == "detached"
        ));
    }

    #[test]
    fn close_returns_to_remaining_surface_then_stashes_when_none_remains() {
        let mut ownership = SurfaceTerminalOwnership::default();
        ownership.observe(observation("main", 1, 1, "", &["/p/a.md"]));
        ownership.observe(observation("detached", 1, 2, "/p/a.md", &["/p/a.md"]));
        assert!(matches!(
            ownership.retire("test", "detached", 1),
            SurfaceTerminalDecision::Mount { surface_id, .. } if surface_id == "main"
        ));
        assert!(matches!(
            ownership.retire("test", "main", 1),
            SurfaceTerminalDecision::Stash { previous }
                if previous.surface_id == "main"
        ));
    }

    #[test]
    fn stale_generation_and_sequence_cannot_move_terminal() {
        let mut ownership = SurfaceTerminalOwnership::default();
        ownership.observe(observation("detached", 4, 9, "/p/a.md", &["/p/a.md"]));
        assert_eq!(
            ownership.observe(observation("detached", 4, 8, "/p/b.md", &["/p/b.md"])),
            SurfaceTerminalDecision::Stale
        );
        assert_eq!(
            ownership.retire("test", "detached", 3),
            SurfaceTerminalDecision::Stale
        );
        assert_eq!(ownership.mounted().unwrap().document, "/p/a.md");
    }

    #[test]
    fn focus_change_in_same_surface_remounts_for_new_document() {
        let mut ownership = SurfaceTerminalOwnership::default();
        ownership.observe(observation(
            "detached",
            1,
            1,
            "/p/a.md",
            &["/p/a.md", "/p/b.md"],
        ));
        assert!(matches!(
            ownership.observe(observation(
                "detached",
                1,
                2,
                "/p/b.md",
                &["/p/a.md", "/p/b.md"],
            )),
            SurfaceTerminalDecision::Mount { document, .. } if document == "/p/b.md"
        ));
    }

    #[test]
    fn passive_projection_cannot_steal_from_authoritative_detached_focus() {
        let mut ownership = SurfaceTerminalOwnership::default();
        let mut detached = observation("detached", 1, 1, "/p/a.md", &["/p/a.md"]);
        detached.source_id = "focus".to_string();
        ownership.observe(detached);

        let mut passive_main = observation("main", 1, 99, "/p/a.md", &["/p/a.md"]);
        passive_main.source_id = "surface".to_string();
        passive_main.authoritative_focus = false;
        assert!(matches!(
            ownership.observe(passive_main),
            SurfaceTerminalDecision::Exclude { owner_surface_id, .. }
                if owner_surface_id == "detached"
        ));
        assert_eq!(ownership.owner_of("/p/a.md"), Some("detached"));
    }
}
