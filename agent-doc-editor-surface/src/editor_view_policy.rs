//! Pure ownership policy for isolated detached editor views (GH #218).
//!
//! The policy consumes complete, authenticated per-client surface snapshots.
//! It owns main-frame precedence, deterministic detached ownership, placeholder
//! decisions, revision fencing, and the bind/release lifecycle. It performs no
//! I/O; callers durably record emitted transitions before applying effects.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::SurfaceColumn;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditorSurfaceRole {
    Main,
    Detached,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EditorViewSurfaceKey {
    pub client_id: String,
    pub connection_generation: u64,
    pub surface_id: String,
    pub surface_generation: u64,
}

impl EditorViewSurfaceKey {
    pub fn view_id(&self) -> EditorViewId {
        EditorViewId {
            client_id: self.client_id.clone(),
            connection_generation: self.connection_generation,
            surface_id: self.surface_id.clone(),
            surface_generation: self.surface_generation,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EditorViewId {
    pub client_id: String,
    pub connection_generation: u64,
    pub surface_id: String,
    pub surface_generation: u64,
}

/// Stable inputs for the bounded isolated tmux session name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorViewSessionKey {
    pub project_id: String,
    pub client_id: String,
    pub connection_generation: u64,
    pub surface_id: String,
    pub surface_generation: u64,
}

pub fn isolated_view_session_name(key: &EditorViewSessionKey) -> String {
    let encoded = format!(
        "{}:{}|{}:{}:{}|{}:{}:{}",
        key.project_id.len(),
        key.project_id,
        key.client_id.len(),
        key.client_id,
        key.connection_generation,
        key.surface_id.len(),
        key.surface_id,
        key.surface_generation,
    );
    format!(
        "agent-doc-view-{}",
        agent_doc_hash::short_content_hash(&encoded)
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorViewSurface {
    pub surface_id: String,
    pub surface_generation: u64,
    pub role: EditorSurfaceRole,
    #[serde(default)]
    pub focused: String,
    #[serde(default)]
    pub visible: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorViewSnapshot {
    pub client_id: String,
    pub connection_generation: u64,
    pub sequence: u64,
    /// Only a complete snapshot may retire omitted surfaces or change policy.
    pub complete: bool,
    /// False when the backend cannot mount a terminal in detached surfaces.
    pub terminal_capable: bool,
    #[serde(default)]
    pub surfaces: Vec<EditorViewSurface>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditorViewBindingPhase {
    BindPending,
    Bound,
    ReleasePending,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorViewPolicyBinding {
    pub document: String,
    pub binding_epoch: u64,
    pub owner: EditorViewSurfaceKey,
    pub view_id: EditorViewId,
    pub phase: EditorViewBindingPhase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditorViewReleaseReason {
    MainVisible,
    OwnerClosed,
    OwnerChangedDocument,
    ClientRetired,
    RecoveryCompensation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditorViewReleaseDestination {
    MainStash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditorViewBindSource {
    MainStash,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EditorViewLifecycleTransition {
    BindPending {
        binding: EditorViewPolicyBinding,
        source: EditorViewBindSource,
    },
    ReleasePending {
        binding: EditorViewPolicyBinding,
        reason: EditorViewReleaseReason,
        destination: EditorViewReleaseDestination,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditorViewPlaceholderReason {
    MainOwned,
    OwnedByOtherDetachedSurface,
    BindingPending,
    ReleasePending,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EditorViewPresentation {
    Empty,
    Terminal {
        document: String,
        view_id: EditorViewId,
    },
    Placeholder {
        document: String,
        reason: EditorViewPlaceholderReason,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        owner: Option<EditorViewSurfaceKey>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EditorViewPolicyStatus {
    Applied,
    Stale,
    Frozen { reason: EditorViewFreezeReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditorViewFreezeReason {
    IncompleteSnapshot,
    MissingMainSurface,
    MultipleMainSurfaces,
    TerminalCapabilityUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorViewPolicyProjection {
    pub status: EditorViewPolicyStatus,
    pub main_visible: BTreeSet<String>,
    pub presentations: BTreeMap<EditorViewSurfaceKey, EditorViewPresentation>,
    pub bindings: BTreeMap<String, EditorViewPolicyBinding>,
    pub transitions: Vec<EditorViewLifecycleTransition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AcceptedClientSnapshot {
    connection_generation: u64,
    surfaces: Vec<EditorViewSurface>,
}

/// Documents allowed to participate in the main layout.
///
/// `BindPending`, `Bound`, and `ReleasePending` are all exclusions. Only a
/// settled release removes the binding from this projection.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MainLayoutEligibility {
    excluded_documents: BTreeSet<String>,
}

impl MainLayoutEligibility {
    pub fn new(documents: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            excluded_documents: documents.into_iter().map(Into::into).collect(),
        }
    }

    pub fn permits(&self, document: &str) -> bool {
        !self.excluded_documents.contains(document)
    }

    pub fn excluded_documents(&self) -> &BTreeSet<String> {
        &self.excluded_documents
    }

    pub fn filter_columns(&self, columns: &[SurfaceColumn]) -> Vec<SurfaceColumn> {
        columns
            .iter()
            .filter_map(|column| {
                let files = column
                    .files
                    .iter()
                    .filter(|document| self.permits(document))
                    .cloned()
                    .collect::<Vec<_>>();
                (!files.is_empty()).then(|| SurfaceColumn::new(files))
            })
            .collect()
    }
}

/// Identity of the process that published a snapshot, as the transport
/// observed it (for example the OS pid of the connecting editor backend).
///
/// It is not part of the snapshot payload: the controller reads it from the
/// connection, so a frontend cannot claim another instance's identity and an
/// already-installed editor needs no new wire field (`#viewsnapshotgenreset`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EditorViewFrontendInstance(pub u64);

/// A revision regression the policy accepted because it came from a new
/// frontend instance rather than from a reordered delivery of the old one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorViewFrontendInstanceReset {
    pub client_id: String,
    pub previous_instance: EditorViewFrontendInstance,
    pub instance: EditorViewFrontendInstance,
    /// `(connection_generation, sequence)` high-water mark of the predecessor.
    pub previous_revision: (u64, u64),
    /// `(connection_generation, sequence)` of the snapshot that reset it.
    pub revision: (u64, u64),
}

/// Result of [`EditorViewPolicy::observe_from_instance`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorViewObservation {
    pub projection: EditorViewPolicyProjection,
    /// Set when this snapshot retired a predecessor frontend instance.
    pub instance_reset: Option<EditorViewFrontendInstanceReset>,
}

/// Sole pure owner of main precedence and detached terminal ownership.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditorViewPolicy {
    clients: BTreeMap<String, AcceptedClientSnapshot>,
    revisions: BTreeMap<String, (u64, u64)>,
    /// Frontend instance that set each client's revision high-water mark, when
    /// the transport could identify it.
    instances: BTreeMap<String, EditorViewFrontendInstance>,
    bindings: BTreeMap<String, EditorViewPolicyBinding>,
    next_binding_epoch: u64,
}

impl EditorViewPolicy {
    pub fn hydrate(bindings: impl IntoIterator<Item = EditorViewPolicyBinding>) -> Self {
        let mut policy = Self::default();
        for binding in bindings {
            policy.next_binding_epoch = policy.next_binding_epoch.max(binding.binding_epoch);
            policy.bindings.insert(binding.document.clone(), binding);
        }
        policy
    }

    pub fn main_layout_eligibility(&self) -> MainLayoutEligibility {
        MainLayoutEligibility::new(self.bindings.keys().cloned())
    }

    /// Observe a snapshot whose publishing process is unknown.
    ///
    /// The `(connection_generation, sequence)` fence is then absolute: any
    /// revision at or below the client's high-water mark is stale.
    pub fn observe(&mut self, snapshot: EditorViewSnapshot) -> EditorViewPolicyProjection {
        self.observe_from_instance(snapshot, None, |_| true)
            .projection
    }

    /// Observe a snapshot published by `instance`.
    ///
    /// The revision fence orders snapshots *within one frontend instance*. An
    /// editor backend restarts its connection generation and sequence with its
    /// process (`#viewsnapshotgenreset`: a restarted JetBrains IDE sent
    /// generation 1 again and every snapshot was stale until its sequence passed
    /// the previous IDE's high-water mark). A regressed revision names a
    /// successor, not a reordered delivery, only when both the high-water
    /// mark's instance and `instance` are known, they differ, and
    /// `predecessor_live` reports the predecessor gone. The predecessor is then
    /// retired (its bindings release as [`EditorViewReleaseReason::ClientRetired`])
    /// and the snapshot is observed against a fresh fence. Otherwise a
    /// regression stays stale: with an identity unknown, from the same instance,
    /// or from a second live process sharing the client id (two IDEs whose
    /// backends both authenticate as the same client), the fence keeps
    /// deciding, so two live publishers cannot reset each other back and forth.
    pub fn observe_from_instance(
        &mut self,
        snapshot: EditorViewSnapshot,
        instance: Option<EditorViewFrontendInstance>,
        predecessor_live: impl FnOnce(EditorViewFrontendInstance) -> bool,
    ) -> EditorViewObservation {
        let incoming = (snapshot.connection_generation, snapshot.sequence);
        let mut retirement_transitions = Vec::new();
        let mut instance_reset = None;
        if let Some(current) = self.revisions.get(&snapshot.client_id).copied()
            && incoming <= current
        {
            let previous_instance = self.instances.get(&snapshot.client_id).copied();
            let (Some(previous_instance), Some(instance)) = (previous_instance, instance) else {
                return self.stale_observation();
            };
            if previous_instance == instance || predecessor_live(previous_instance) {
                return self.stale_observation();
            }
            self.revisions.remove(&snapshot.client_id);
            self.instances.remove(&snapshot.client_id);
            if self.clients.remove(&snapshot.client_id).is_some() {
                retirement_transitions = self
                    .recompute(Some(EditorViewReleaseReason::ClientRetired))
                    .transitions;
            }
            instance_reset = Some(EditorViewFrontendInstanceReset {
                client_id: snapshot.client_id.clone(),
                previous_instance,
                instance,
                previous_revision: current,
                revision: incoming,
            });
        }
        self.revisions.insert(snapshot.client_id.clone(), incoming);
        match instance {
            Some(instance) => {
                self.instances.insert(snapshot.client_id.clone(), instance);
            }
            None => {
                self.instances.remove(&snapshot.client_id);
            }
        }

        let mut projection = match validate_snapshot(&snapshot) {
            Some(reason) => self.project(EditorViewPolicyStatus::Frozen { reason }, Vec::new()),
            None => {
                self.clients.insert(
                    snapshot.client_id,
                    AcceptedClientSnapshot {
                        connection_generation: snapshot.connection_generation,
                        surfaces: snapshot.surfaces,
                    },
                );
                self.recompute(None)
            }
        };
        if !retirement_transitions.is_empty() {
            retirement_transitions.append(&mut projection.transitions);
            projection.transitions = retirement_transitions;
        }
        EditorViewObservation {
            projection,
            instance_reset,
        }
    }

    fn stale_observation(&self) -> EditorViewObservation {
        EditorViewObservation {
            projection: self.project(EditorViewPolicyStatus::Stale, Vec::new()),
            instance_reset: None,
        }
    }

    pub fn retire_client(
        &mut self,
        client_id: &str,
        connection_generation: u64,
    ) -> EditorViewPolicyProjection {
        let Some(current) = self.clients.get(client_id) else {
            return self.project(EditorViewPolicyStatus::Stale, Vec::new());
        };
        if current.connection_generation != connection_generation {
            return self.project(EditorViewPolicyStatus::Stale, Vec::new());
        }
        self.clients.remove(client_id);
        self.recompute(Some(EditorViewReleaseReason::ClientRetired))
    }

    pub fn settle_bound(
        &mut self,
        document: &str,
        binding_epoch: u64,
        view_id: &EditorViewId,
    ) -> EditorViewPolicyProjection {
        let Some(binding) = self.bindings.get_mut(document) else {
            return self.project(EditorViewPolicyStatus::Stale, Vec::new());
        };
        if binding.binding_epoch != binding_epoch
            || &binding.view_id != view_id
            || binding.phase != EditorViewBindingPhase::BindPending
        {
            return self.project(EditorViewPolicyStatus::Stale, Vec::new());
        }
        binding.phase = EditorViewBindingPhase::Bound;
        // Settling the tmux receipt is not new editor evidence. In particular,
        // restart recovery may finish a pending bind before the first complete
        // frontend snapshot arrives; recomputing against an empty client set
        // would immediately misclassify the recovered owner as closed.
        self.project(EditorViewPolicyStatus::Applied, Vec::new())
    }

    pub fn settle_released(
        &mut self,
        document: &str,
        binding_epoch: u64,
        view_id: &EditorViewId,
    ) -> EditorViewPolicyProjection {
        let Some(binding) = self.bindings.get(document) else {
            return self.project(EditorViewPolicyStatus::Stale, Vec::new());
        };
        if binding.binding_epoch != binding_epoch
            || &binding.view_id != view_id
            || binding.phase != EditorViewBindingPhase::ReleasePending
        {
            return self.project(EditorViewPolicyStatus::Stale, Vec::new());
        }
        self.bindings.remove(document);
        self.recompute(None)
    }

    fn recompute(
        &mut self,
        forced_release_reason: Option<EditorViewReleaseReason>,
    ) -> EditorViewPolicyProjection {
        let main_visible = self.main_visible_documents();
        let candidates = self.detached_candidates();
        let live_surfaces = self
            .detached_focuses()
            .into_iter()
            .map(|(surface, _)| surface)
            .collect::<BTreeSet<_>>();
        let mut transitions = Vec::new();

        for binding in self.bindings.values_mut() {
            if binding.phase == EditorViewBindingPhase::ReleasePending {
                continue;
            }
            let reason = if main_visible.contains(&binding.document) {
                Some(EditorViewReleaseReason::MainVisible)
            } else if !candidates
                .get(&binding.document)
                .is_some_and(|owners| owners.contains(&binding.owner))
            {
                Some(forced_release_reason.unwrap_or_else(|| {
                    if live_surfaces.contains(&binding.owner) {
                        EditorViewReleaseReason::OwnerChangedDocument
                    } else {
                        EditorViewReleaseReason::OwnerClosed
                    }
                }))
            } else {
                None
            };
            if let Some(reason) = reason {
                binding.phase = EditorViewBindingPhase::ReleasePending;
                transitions.push(EditorViewLifecycleTransition::ReleasePending {
                    binding: binding.clone(),
                    reason,
                    destination: EditorViewReleaseDestination::MainStash,
                });
            }
        }

        let reserved_surfaces = self
            .bindings
            .values()
            .map(|binding| binding.owner.clone())
            .collect::<BTreeSet<_>>();
        for (document, owners) in &candidates {
            if main_visible.contains(document) || self.bindings.contains_key(document) {
                continue;
            }
            let Some(owner) = owners
                .iter()
                .find(|owner| !reserved_surfaces.contains(*owner))
                .cloned()
            else {
                continue;
            };
            self.next_binding_epoch = self.next_binding_epoch.saturating_add(1);
            let binding = EditorViewPolicyBinding {
                document: document.clone(),
                binding_epoch: self.next_binding_epoch,
                view_id: owner.view_id(),
                owner,
                phase: EditorViewBindingPhase::BindPending,
            };
            self.bindings.insert(document.clone(), binding.clone());
            transitions.push(EditorViewLifecycleTransition::BindPending {
                binding,
                source: EditorViewBindSource::MainStash,
            });
        }

        self.project(EditorViewPolicyStatus::Applied, transitions)
    }

    fn project(
        &self,
        status: EditorViewPolicyStatus,
        transitions: Vec<EditorViewLifecycleTransition>,
    ) -> EditorViewPolicyProjection {
        let main_visible = self.main_visible_documents();
        let mut presentations = BTreeMap::new();
        for (surface, document) in self.detached_focuses() {
            let presentation = if document.is_empty() {
                EditorViewPresentation::Empty
            } else if main_visible.contains(&document) {
                EditorViewPresentation::Placeholder {
                    document,
                    reason: EditorViewPlaceholderReason::MainOwned,
                    owner: None,
                }
            } else if let Some(binding) = self.bindings.get(&document) {
                if binding.owner != surface {
                    EditorViewPresentation::Placeholder {
                        document,
                        reason: EditorViewPlaceholderReason::OwnedByOtherDetachedSurface,
                        owner: Some(binding.owner.clone()),
                    }
                } else {
                    match binding.phase {
                        EditorViewBindingPhase::Bound => EditorViewPresentation::Terminal {
                            document,
                            view_id: binding.view_id.clone(),
                        },
                        EditorViewBindingPhase::BindPending => {
                            EditorViewPresentation::Placeholder {
                                document,
                                reason: EditorViewPlaceholderReason::BindingPending,
                                owner: Some(surface.clone()),
                            }
                        }
                        EditorViewBindingPhase::ReleasePending => {
                            EditorViewPresentation::Placeholder {
                                document,
                                reason: EditorViewPlaceholderReason::ReleasePending,
                                owner: Some(surface.clone()),
                            }
                        }
                    }
                }
            } else {
                EditorViewPresentation::Empty
            };
            presentations.insert(surface, presentation);
        }
        EditorViewPolicyProjection {
            status,
            main_visible,
            presentations,
            bindings: self.bindings.clone(),
            transitions,
        }
    }

    fn main_visible_documents(&self) -> BTreeSet<String> {
        self.clients
            .iter()
            .flat_map(|(client_id, client)| {
                client.surfaces.iter().filter_map(move |surface| {
                    (surface.role == EditorSurfaceRole::Main)
                        .then_some((client_id, client, surface))
                })
            })
            .flat_map(|(_, _, surface)| surface.visible.iter().cloned())
            .filter(|document| !document.is_empty())
            .collect()
    }

    fn detached_candidates(&self) -> BTreeMap<String, BTreeSet<EditorViewSurfaceKey>> {
        let mut candidates: BTreeMap<String, BTreeSet<EditorViewSurfaceKey>> = BTreeMap::new();
        for (surface, document) in self.detached_focuses() {
            if !document.is_empty() {
                candidates.entry(document).or_default().insert(surface);
            }
        }
        candidates
    }

    fn detached_focuses(&self) -> Vec<(EditorViewSurfaceKey, String)> {
        self.clients
            .iter()
            .flat_map(|(client_id, client)| {
                client
                    .surfaces
                    .iter()
                    .filter(|surface| surface.role == EditorSurfaceRole::Detached)
                    .map(move |surface| {
                        let focused = if surface.visible.contains(&surface.focused) {
                            surface.focused.clone()
                        } else {
                            String::new()
                        };
                        (
                            EditorViewSurfaceKey {
                                client_id: client_id.clone(),
                                connection_generation: client.connection_generation,
                                surface_id: surface.surface_id.clone(),
                                surface_generation: surface.surface_generation,
                            },
                            focused,
                        )
                    })
            })
            .collect()
    }
}

fn validate_snapshot(snapshot: &EditorViewSnapshot) -> Option<EditorViewFreezeReason> {
    if !snapshot.complete {
        return Some(EditorViewFreezeReason::IncompleteSnapshot);
    }
    let main_count = snapshot
        .surfaces
        .iter()
        .filter(|surface| surface.role == EditorSurfaceRole::Main)
        .count();
    if main_count == 0 {
        return Some(EditorViewFreezeReason::MissingMainSurface);
    }
    if main_count > 1 {
        return Some(EditorViewFreezeReason::MultipleMainSurfaces);
    }
    if !snapshot.terminal_capable
        && snapshot
            .surfaces
            .iter()
            .any(|surface| surface.role == EditorSurfaceRole::Detached)
    {
        return Some(EditorViewFreezeReason::TerminalCapabilityUnavailable);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn surface(
        id: &str,
        role: EditorSurfaceRole,
        focused: &str,
        visible: &[&str],
    ) -> EditorViewSurface {
        EditorViewSurface {
            surface_id: id.to_string(),
            surface_generation: 1,
            role,
            focused: focused.to_string(),
            visible: visible.iter().map(|value| (*value).to_string()).collect(),
        }
    }

    fn snapshot(
        client: &str,
        generation: u64,
        sequence: u64,
        surfaces: Vec<EditorViewSurface>,
    ) -> EditorViewSnapshot {
        EditorViewSnapshot {
            client_id: client.to_string(),
            connection_generation: generation,
            sequence,
            complete: true,
            terminal_capable: true,
            surfaces,
        }
    }

    fn first_binding(projection: &EditorViewPolicyProjection) -> EditorViewPolicyBinding {
        projection.bindings.values().next().unwrap().clone()
    }

    #[test]
    fn detached_only_document_requests_binding_then_settles_terminal() {
        let mut policy = EditorViewPolicy::default();
        let projection = policy.observe(snapshot(
            "client-a",
            1,
            1,
            vec![
                surface("root", EditorSurfaceRole::Main, "", &[]),
                surface(
                    "dock-1",
                    EditorSurfaceRole::Detached,
                    "/p/a.md",
                    &["/p/a.md"],
                ),
            ],
        ));
        let binding = first_binding(&projection);
        assert_eq!(binding.phase, EditorViewBindingPhase::BindPending);
        assert!(matches!(
            projection.transitions.as_slice(),
            [EditorViewLifecycleTransition::BindPending { .. }]
        ));
        assert!(!policy.main_layout_eligibility().permits("/p/a.md"));

        let projection = policy.settle_bound("/p/a.md", binding.binding_epoch, &binding.view_id);
        assert!(matches!(
            projection.presentations.get(&binding.owner),
            Some(EditorViewPresentation::Terminal { document, .. }) if document == "/p/a.md"
        ));
        let unchanged = policy.observe(snapshot(
            "client-a",
            1,
            2,
            vec![
                surface("root", EditorSurfaceRole::Main, "", &[]),
                surface(
                    "dock-1",
                    EditorSurfaceRole::Detached,
                    "/p/a.md",
                    &["/p/a.md"],
                ),
            ],
        ));
        assert!(unchanged.transitions.is_empty());
        assert_eq!(
            unchanged.bindings["/p/a.md"].phase,
            EditorViewBindingPhase::Bound
        );
    }

    #[test]
    fn main_visibility_wins_without_ever_requesting_a_detached_bind() {
        let mut policy = EditorViewPolicy::default();
        let projection = policy.observe(snapshot(
            "client-a",
            1,
            1,
            vec![
                surface("root", EditorSurfaceRole::Main, "/p/a.md", &["/p/a.md"]),
                surface(
                    "dock-1",
                    EditorSurfaceRole::Detached,
                    "/p/a.md",
                    &["/p/a.md"],
                ),
            ],
        ));
        assert!(projection.bindings.is_empty());
        assert!(projection.transitions.is_empty());
        assert!(matches!(
            projection.presentations.values().next(),
            Some(EditorViewPresentation::Placeholder {
                reason: EditorViewPlaceholderReason::MainOwned,
                ..
            })
        ));
    }

    #[test]
    fn main_becoming_visible_releases_bound_view_before_main_is_eligible() {
        let mut policy = EditorViewPolicy::default();
        let first = policy.observe(snapshot(
            "client-a",
            1,
            1,
            vec![
                surface("root", EditorSurfaceRole::Main, "", &[]),
                surface(
                    "dock-1",
                    EditorSurfaceRole::Detached,
                    "/p/a.md",
                    &["/p/a.md"],
                ),
            ],
        ));
        let binding = first_binding(&first);
        policy.settle_bound("/p/a.md", binding.binding_epoch, &binding.view_id);

        let release = policy.observe(snapshot(
            "client-a",
            1,
            2,
            vec![
                surface("root", EditorSurfaceRole::Main, "/p/a.md", &["/p/a.md"]),
                surface(
                    "dock-1",
                    EditorSurfaceRole::Detached,
                    "/p/a.md",
                    &["/p/a.md"],
                ),
            ],
        ));
        assert!(matches!(
            release.transitions.as_slice(),
            [EditorViewLifecycleTransition::ReleasePending {
                reason: EditorViewReleaseReason::MainVisible,
                ..
            }]
        ));
        assert!(!policy.main_layout_eligibility().permits("/p/a.md"));
        let released = policy.settle_released("/p/a.md", binding.binding_epoch, &binding.view_id);
        assert!(released.bindings.is_empty());
        assert!(policy.main_layout_eligibility().permits("/p/a.md"));
    }

    #[test]
    fn closing_owner_releases_to_main_stash() {
        let mut policy = EditorViewPolicy::default();
        let first = policy.observe(snapshot(
            "client-a",
            1,
            1,
            vec![
                surface("root", EditorSurfaceRole::Main, "", &[]),
                surface(
                    "dock-1",
                    EditorSurfaceRole::Detached,
                    "/p/a.md",
                    &["/p/a.md"],
                ),
            ],
        ));
        let binding = first_binding(&first);
        policy.settle_bound("/p/a.md", binding.binding_epoch, &binding.view_id);
        let closed = policy.observe(snapshot(
            "client-a",
            1,
            2,
            vec![surface("root", EditorSurfaceRole::Main, "", &[])],
        ));
        assert!(matches!(
            closed.transitions.as_slice(),
            [EditorViewLifecycleTransition::ReleasePending {
                reason: EditorViewReleaseReason::OwnerClosed,
                destination: EditorViewReleaseDestination::MainStash,
                ..
            }]
        ));
    }

    #[test]
    fn duplicate_detached_document_keeps_deterministic_owner_and_placeholder() {
        let mut policy = EditorViewPolicy::default();
        let projection = policy.observe(snapshot(
            "client-a",
            1,
            1,
            vec![
                surface("root", EditorSurfaceRole::Main, "", &[]),
                surface(
                    "dock-b",
                    EditorSurfaceRole::Detached,
                    "/p/a.md",
                    &["/p/a.md"],
                ),
                surface(
                    "dock-a",
                    EditorSurfaceRole::Detached,
                    "/p/a.md",
                    &["/p/a.md"],
                ),
            ],
        ));
        let binding = first_binding(&projection);
        assert_eq!(binding.owner.surface_id, "dock-a");
        assert!(
            projection
                .presentations
                .iter()
                .any(|(surface, presentation)| {
                    surface.surface_id == "dock-b"
                        && matches!(
                            presentation,
                            EditorViewPresentation::Placeholder {
                                reason: EditorViewPlaceholderReason::OwnedByOtherDetachedSurface,
                                ..
                            }
                        )
                })
        );
    }

    #[test]
    fn hydrated_active_bindings_are_excluded_and_not_reissued() {
        let owner = EditorViewSurfaceKey {
            client_id: "client-a".to_string(),
            connection_generation: 4,
            surface_id: "dock-1".to_string(),
            surface_generation: 2,
        };
        for phase in [
            EditorViewBindingPhase::BindPending,
            EditorViewBindingPhase::Bound,
            EditorViewBindingPhase::ReleasePending,
        ] {
            let binding = EditorViewPolicyBinding {
                document: "/p/a.md".to_string(),
                binding_epoch: 9,
                view_id: owner.view_id(),
                owner: owner.clone(),
                phase,
            };
            let mut policy = EditorViewPolicy::hydrate([binding.clone()]);
            let projection = policy.observe(EditorViewSnapshot {
                client_id: "client-a".to_string(),
                connection_generation: 4,
                sequence: 1,
                complete: true,
                terminal_capable: true,
                surfaces: vec![
                    surface("root", EditorSurfaceRole::Main, "", &[]),
                    EditorViewSurface {
                        surface_generation: 2,
                        ..surface(
                            "dock-1",
                            EditorSurfaceRole::Detached,
                            "/p/a.md",
                            &["/p/a.md"],
                        )
                    },
                ],
            });
            assert!(projection.transitions.is_empty());
            assert_eq!(projection.bindings.get("/p/a.md"), Some(&binding));
            assert!(!policy.main_layout_eligibility().permits("/p/a.md"));
        }
    }

    #[test]
    fn stale_and_incomplete_snapshots_cannot_change_policy() {
        let mut policy = EditorViewPolicy::default();
        let accepted = snapshot(
            "client-a",
            2,
            5,
            vec![surface(
                "root",
                EditorSurfaceRole::Main,
                "/p/main.md",
                &["/p/main.md"],
            )],
        );
        policy.observe(accepted);
        let stale = policy.observe(snapshot(
            "client-a",
            2,
            4,
            vec![surface(
                "root",
                EditorSurfaceRole::Main,
                "/p/stale.md",
                &["/p/stale.md"],
            )],
        ));
        assert_eq!(stale.status, EditorViewPolicyStatus::Stale);
        assert!(stale.main_visible.contains("/p/main.md"));

        let mut incomplete = snapshot(
            "client-a",
            2,
            6,
            vec![surface(
                "root",
                EditorSurfaceRole::Main,
                "/p/new.md",
                &["/p/new.md"],
            )],
        );
        incomplete.complete = false;
        let frozen = policy.observe(incomplete);
        assert_eq!(
            frozen.status,
            EditorViewPolicyStatus::Frozen {
                reason: EditorViewFreezeReason::IncompleteSnapshot
            }
        );
        assert!(frozen.main_visible.contains("/p/main.md"));
    }

    fn main_only(client: &str, generation: u64, sequence: u64, doc: &str) -> EditorViewSnapshot {
        snapshot(
            client,
            generation,
            sequence,
            vec![surface("root", EditorSurfaceRole::Main, doc, &[doc])],
        )
    }

    const OLD_IDE: EditorViewFrontendInstance = EditorViewFrontendInstance(4101);
    const NEW_IDE: EditorViewFrontendInstance = EditorViewFrontendInstance(5202);

    fn dead(_: EditorViewFrontendInstance) -> bool {
        false
    }

    fn live(_: EditorViewFrontendInstance) -> bool {
        true
    }

    /// `#viewsnapshotgenreset`: a restarted editor backend restarts its
    /// connection generation and sequence at 1. A regressed revision from a
    /// different process is a successor, applied at once, not a stale replay
    /// that waits for the sequence to pass the predecessor's high-water mark.
    #[test]
    fn regressed_revision_from_a_new_frontend_instance_resets_the_fence() {
        let mut policy = EditorViewPolicy::default();
        let accepted = policy.observe_from_instance(
            main_only("rd:Host", 1, 1231, "/p/old.md"),
            Some(OLD_IDE),
            dead,
        );
        assert_eq!(accepted.projection.status, EditorViewPolicyStatus::Applied);
        assert_eq!(accepted.instance_reset, None);

        let restarted = policy.observe_from_instance(
            main_only("rd:Host", 1, 27, "/p/new.md"),
            Some(NEW_IDE),
            dead,
        );
        assert_eq!(restarted.projection.status, EditorViewPolicyStatus::Applied);
        assert_eq!(
            restarted.instance_reset,
            Some(EditorViewFrontendInstanceReset {
                client_id: "rd:Host".to_string(),
                previous_instance: OLD_IDE,
                instance: NEW_IDE,
                previous_revision: (1, 1231),
                revision: (1, 27),
            })
        );
        assert!(restarted.projection.main_visible.contains("/p/new.md"));
        assert!(!restarted.projection.main_visible.contains("/p/old.md"));

        // The fence now orders the successor's own deliveries.
        let next = policy.observe_from_instance(
            main_only("rd:Host", 1, 28, "/p/next.md"),
            Some(NEW_IDE),
            dead,
        );
        assert_eq!(next.projection.status, EditorViewPolicyStatus::Applied);
        assert_eq!(next.instance_reset, None);
        let replay = policy.observe_from_instance(
            main_only("rd:Host", 1, 27, "/p/new.md"),
            Some(NEW_IDE),
            dead,
        );
        assert_eq!(replay.projection.status, EditorViewPolicyStatus::Stale);
        assert!(replay.projection.main_visible.contains("/p/next.md"));
        let same = policy.observe_from_instance(
            main_only("rd:Host", 1, 5, "/p/x.md"),
            Some(NEW_IDE),
            dead,
        );
        assert_eq!(same.projection.status, EditorViewPolicyStatus::Stale);
    }

    /// Without a transport identity on either side the fence stays absolute, so
    /// a reordered delivery can never roll the policy back.
    #[test]
    fn regressed_revision_without_instance_identity_stays_stale() {
        for (previous, incoming) in [
            (None, None),
            (Some(OLD_IDE), None),
            (None, Some(NEW_IDE)),
            (Some(OLD_IDE), Some(OLD_IDE)),
        ] {
            let mut policy = EditorViewPolicy::default();
            policy.observe_from_instance(main_only("rd:Host", 1, 40, "/p/main.md"), previous, dead);
            for regressed in [(1, 40), (1, 39), (1, 1)] {
                let stale = policy.observe_from_instance(
                    main_only("rd:Host", regressed.0, regressed.1, "/p/stale.md"),
                    incoming,
                    dead,
                );
                assert_eq!(
                    stale.projection.status,
                    EditorViewPolicyStatus::Stale,
                    "{previous:?} -> {incoming:?} at {regressed:?}"
                );
                assert_eq!(stale.instance_reset, None);
                assert!(stale.projection.main_visible.contains("/p/main.md"));
            }
        }
    }

    /// The successor retires its predecessor: the dead instance's detached
    /// binding releases as `ClientRetired` even when the successor reuses the
    /// same surface key (both processes start at generation 1, surface 1).
    #[test]
    fn instance_reset_retires_the_predecessor_bindings() {
        let detached = |sequence: u64, focused: &str| {
            snapshot(
                "rd:Host",
                1,
                sequence,
                vec![
                    surface("root", EditorSurfaceRole::Main, "", &[]),
                    surface("dock-1", EditorSurfaceRole::Detached, focused, &[focused]),
                ],
            )
        };
        let mut policy = EditorViewPolicy::default();
        let bound = policy.observe_from_instance(detached(900, "/p/a.md"), Some(OLD_IDE), dead);
        let binding = first_binding(&bound.projection);
        assert_eq!(binding.phase, EditorViewBindingPhase::BindPending);

        let restarted = policy.observe_from_instance(detached(3, "/p/a.md"), Some(NEW_IDE), dead);
        assert_eq!(restarted.projection.status, EditorViewPolicyStatus::Applied);
        assert!(restarted.instance_reset.is_some());
        assert_eq!(
            restarted.projection.transitions,
            vec![EditorViewLifecycleTransition::ReleasePending {
                binding: EditorViewPolicyBinding {
                    phase: EditorViewBindingPhase::ReleasePending,
                    ..binding
                },
                reason: EditorViewReleaseReason::ClientRetired,
                destination: EditorViewReleaseDestination::MainStash,
            }]
        );
    }

    /// Two live processes sharing one client id (two IDEs whose backends both
    /// authenticate as `Host`) must not reset each other back and forth: while
    /// the fence's owner is alive, the fence still decides.
    #[test]
    fn regression_from_another_live_instance_stays_stale() {
        let mut policy = EditorViewPolicy::default();
        policy.observe_from_instance(
            main_only("rd:Host", 1, 40, "/p/main.md"),
            Some(OLD_IDE),
            dead,
        );
        let mut asked = None;
        let stale = policy.observe_from_instance(
            main_only("rd:Host", 1, 3, "/p/other.md"),
            Some(NEW_IDE),
            |predecessor| {
                asked = Some(predecessor);
                live(predecessor)
            },
        );
        assert_eq!(asked, Some(OLD_IDE));
        assert_eq!(stale.projection.status, EditorViewPolicyStatus::Stale);
        assert_eq!(stale.instance_reset, None);
        assert!(stale.projection.main_visible.contains("/p/main.md"));
    }

    /// A newer revision from a new instance needs no reset; it is simply newer,
    /// and the liveness probe is never consulted for an advancing revision.
    #[test]
    fn advancing_revision_from_a_new_instance_is_not_a_reset() {
        let mut policy = EditorViewPolicy::default();
        policy.observe_from_instance(main_only("rd:Host", 1, 10, "/p/a.md"), Some(OLD_IDE), dead);
        let advanced = policy.observe_from_instance(
            main_only("rd:Host", 2, 1, "/p/b.md"),
            Some(NEW_IDE),
            |_| unreachable!("an advancing revision needs no predecessor probe"),
        );
        assert_eq!(advanced.projection.status, EditorViewPolicyStatus::Applied);
        assert_eq!(advanced.instance_reset, None);
        // Clients are fenced independently.
        let unrelated =
            policy.observe_from_instance(main_only("other", 1, 1, "/p/c.md"), Some(OLD_IDE), dead);
        assert_eq!(unrelated.projection.status, EditorViewPolicyStatus::Applied);
        assert_eq!(unrelated.instance_reset, None);
    }

    #[test]
    fn malformed_roles_and_missing_terminal_capability_fail_closed() {
        let cases = [
            (
                vec![surface(
                    "dock",
                    EditorSurfaceRole::Detached,
                    "/p/a.md",
                    &["/p/a.md"],
                )],
                true,
                EditorViewFreezeReason::MissingMainSurface,
            ),
            (
                vec![
                    surface("root-a", EditorSurfaceRole::Main, "", &[]),
                    surface("root-b", EditorSurfaceRole::Main, "", &[]),
                ],
                true,
                EditorViewFreezeReason::MultipleMainSurfaces,
            ),
            (
                vec![
                    surface("root", EditorSurfaceRole::Main, "", &[]),
                    surface("dock", EditorSurfaceRole::Detached, "/p/a.md", &["/p/a.md"]),
                ],
                false,
                EditorViewFreezeReason::TerminalCapabilityUnavailable,
            ),
        ];
        for (surfaces, terminal_capable, reason) in cases {
            let mut policy = EditorViewPolicy::default();
            let mut observation = snapshot("client-a", 1, 1, surfaces);
            observation.terminal_capable = terminal_capable;
            assert_eq!(
                policy.observe(observation).status,
                EditorViewPolicyStatus::Frozen { reason }
            );
        }
    }

    #[test]
    fn identical_surface_ids_from_two_clients_do_not_collide() {
        let mut policy = EditorViewPolicy::default();
        policy.observe(snapshot(
            "client-a",
            1,
            1,
            vec![
                surface("root", EditorSurfaceRole::Main, "", &[]),
                surface(
                    "dock-1",
                    EditorSurfaceRole::Detached,
                    "/p/a.md",
                    &["/p/a.md"],
                ),
            ],
        ));
        let projection = policy.observe(snapshot(
            "client-b",
            1,
            1,
            vec![
                surface("root", EditorSurfaceRole::Main, "", &[]),
                surface(
                    "dock-1",
                    EditorSurfaceRole::Detached,
                    "/p/b.md",
                    &["/p/b.md"],
                ),
            ],
        ));
        assert_eq!(projection.bindings.len(), 2);
        let owners = projection
            .bindings
            .values()
            .map(|binding| binding.owner.client_id.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(owners, BTreeSet::from(["client-a", "client-b"]));
    }

    #[test]
    fn reconnect_releases_the_retired_owner_and_fences_old_generation() {
        let mut policy = EditorViewPolicy::default();
        let first = policy.observe(snapshot(
            "client-a",
            1,
            8,
            vec![
                surface("root", EditorSurfaceRole::Main, "", &[]),
                surface("dock", EditorSurfaceRole::Detached, "/p/a.md", &["/p/a.md"]),
            ],
        ));
        let binding = first_binding(&first);
        policy.settle_bound("/p/a.md", binding.binding_epoch, &binding.view_id);

        let reconnected = policy.observe(snapshot(
            "client-a",
            2,
            1,
            vec![
                surface("root", EditorSurfaceRole::Main, "", &[]),
                surface("dock", EditorSurfaceRole::Detached, "/p/a.md", &["/p/a.md"]),
            ],
        ));
        assert!(matches!(
            reconnected.transitions.as_slice(),
            [EditorViewLifecycleTransition::ReleasePending {
                reason: EditorViewReleaseReason::OwnerClosed,
                ..
            }]
        ));

        let stale = policy.observe(snapshot(
            "client-a",
            1,
            99,
            vec![surface(
                "root",
                EditorSurfaceRole::Main,
                "/p/b.md",
                &["/p/b.md"],
            )],
        ));
        assert_eq!(stale.status, EditorViewPolicyStatus::Stale);
        assert!(!stale.main_visible.contains("/p/b.md"));

        let rebound = policy.settle_released("/p/a.md", binding.binding_epoch, &binding.view_id);
        let replacement = first_binding(&rebound);
        assert_eq!(replacement.owner.connection_generation, 2);
        assert!(replacement.binding_epoch > binding.binding_epoch);
    }

    #[test]
    fn stale_settlement_receipts_cannot_change_ownership_or_eligibility() {
        let mut policy = EditorViewPolicy::default();
        let first = policy.observe(snapshot(
            "client-a",
            1,
            1,
            vec![
                surface("root", EditorSurfaceRole::Main, "", &[]),
                surface("dock", EditorSurfaceRole::Detached, "/p/a.md", &["/p/a.md"]),
            ],
        ));
        let binding = first_binding(&first);
        assert_eq!(
            policy
                .settle_bound("/p/a.md", binding.binding_epoch + 1, &binding.view_id)
                .status,
            EditorViewPolicyStatus::Stale
        );
        assert_eq!(
            policy.main_layout_eligibility().excluded_documents(),
            &BTreeSet::from(["/p/a.md".to_string()])
        );
        policy.settle_bound("/p/a.md", binding.binding_epoch, &binding.view_id);
        policy.observe(snapshot(
            "client-a",
            1,
            2,
            vec![
                surface("root", EditorSurfaceRole::Main, "/p/a.md", &["/p/a.md"]),
                surface("dock", EditorSurfaceRole::Detached, "/p/a.md", &["/p/a.md"]),
            ],
        ));
        let wrong_view = EditorViewId {
            surface_id: "other".to_string(),
            ..binding.view_id.clone()
        };
        assert_eq!(
            policy
                .settle_released("/p/a.md", binding.binding_epoch, &wrong_view)
                .status,
            EditorViewPolicyStatus::Stale
        );
        assert!(!policy.main_layout_eligibility().permits("/p/a.md"));
        policy.settle_released("/p/a.md", binding.binding_epoch, &binding.view_id);
        assert!(policy.main_layout_eligibility().permits("/p/a.md"));
    }

    #[test]
    fn focus_switch_waits_for_release_before_binding_replacement() {
        let mut policy = EditorViewPolicy::default();
        let first = policy.observe(snapshot(
            "client-a",
            1,
            1,
            vec![
                surface("root", EditorSurfaceRole::Main, "", &[]),
                surface("dock", EditorSurfaceRole::Detached, "/p/a.md", &["/p/a.md"]),
            ],
        ));
        let binding = first_binding(&first);
        policy.settle_bound("/p/a.md", binding.binding_epoch, &binding.view_id);
        let switched = policy.observe(snapshot(
            "client-a",
            1,
            2,
            vec![
                surface("root", EditorSurfaceRole::Main, "", &[]),
                surface("dock", EditorSurfaceRole::Detached, "/p/b.md", &["/p/b.md"]),
            ],
        ));
        assert!(switched.bindings.contains_key("/p/a.md"));
        assert!(!switched.bindings.contains_key("/p/b.md"));

        let rebound = policy.settle_released("/p/a.md", binding.binding_epoch, &binding.view_id);
        assert!(rebound.bindings.contains_key("/p/b.md"));
        assert!(matches!(
            rebound.transitions.as_slice(),
            [EditorViewLifecycleTransition::BindPending { binding, source }]
                if binding.document == "/p/b.md" && *source == EditorViewBindSource::MainStash
        ));
    }

    #[test]
    fn main_layout_eligibility_filters_files_and_empty_columns() {
        let eligibility = MainLayoutEligibility::new(["/p/view.md"]);
        let filtered = eligibility.filter_columns(&[
            SurfaceColumn::new(["/p/main.md", "/p/view.md"]),
            SurfaceColumn::new(["/p/view.md"]),
        ]);
        assert_eq!(filtered, vec![SurfaceColumn::new(["/p/main.md"])]);
    }
}
