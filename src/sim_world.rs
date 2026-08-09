//! Test-only deterministic workflow simulator.
//!
//! The simulator deliberately stays small: it models the closeout state that is
//! cheap to exercise in memory, and delegates document semantics to production
//! parsers/classifiers wherever possible.

use anyhow::{Result, anyhow, bail};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const FAST_CORPUS_SEEDS: std::ops::Range<u64> = 0..512;
const FAST_CORPUS_STEPS: usize = 24;
const FAST_CORPUS_BUDGET: Duration = Duration::from_secs(3);
const MEDIUM_CORPUS_SEEDS: std::ops::Range<u64> = 0..2_048;
const MEDIUM_CORPUS_STEPS: usize = 32;
const MEDIUM_CORPUS_BUDGET: Duration = Duration::from_secs(12);

/// `#pane-layout-reactive-latest` reference model: a newer desired layout
/// published while the current effect is finishing remains owned by one active
/// worker. The production IO layer adds a Condvar wake; this model exercises the
/// pure publish/retire boundary across the race.
mod pane_layout_projection_model {
    use agent_doc_controller::pane_layout::LatestProjectionWorkerState;

    #[derive(Clone, Copy)]
    enum Action {
        Publish(u64),
        Finish(u64),
    }

    #[derive(Default)]
    struct World {
        worker: LatestProjectionWorkerState,
        starts: usize,
    }

    impl World {
        fn step(&mut self, action: Action) {
            match action {
                Action::Publish(generation) => {
                    self.starts += usize::from(self.worker.schedule(generation));
                }
                Action::Finish(generation) => {
                    self.worker.retire_if_current(generation);
                }
            }
        }
    }

    #[test]
    fn newer_layout_published_at_worker_exit_is_not_stranded_idle() {
        let mut world = World::default();
        world.step(Action::Publish(41));
        world.step(Action::Publish(42));
        world.step(Action::Finish(41));

        assert!(world.worker.is_active());
        assert_eq!(world.worker.pending_revision(), 42);
        assert_eq!(
            world.starts, 1,
            "new state coalesces into the active worker"
        );

        world.step(Action::Finish(42));
        assert!(!world.worker.is_active());
    }

    #[test]
    fn publication_after_idle_starts_exactly_one_new_worker() {
        let mut world = World::default();
        world.step(Action::Publish(51));
        world.step(Action::Finish(51));
        world.step(Action::Publish(52));

        assert!(world.worker.is_active());
        assert_eq!(world.worker.pending_revision(), 52);
        assert_eq!(world.starts, 2);
    }
}

/// `#panewindowdrift` reference model: a layout column whose pane was moved into
/// the stash window after it was bound.
///
/// The file→pane assignment and the actor/registry records all keep the window
/// captured at bind time, so every record-vs-record comparison still agrees and
/// the drift is invisible. Only the live window can see it, which is why the
/// focus effect compares the layout's target window against where the pane
/// actually is. The model drives the production predicate
/// `agent_doc_controller::pane_layout::pane_window_binding_drifted` and carries
/// its own record-only mutation so the coverage is provably sensitive.
mod pane_layout_stashed_column_model {
    use agent_doc_controller::pane_layout::pane_window_binding_drifted;
    use std::collections::BTreeMap;

    const LAYOUT_WINDOW: &str = "@894";
    const STASH_WINDOW: &str = "@904";

    #[derive(Clone, Copy)]
    enum Action {
        /// The operator (or a reconcile) moves a column's pane into the stash
        /// window. Neither the layout assignment nor the bind-time record moves
        /// with it — that is the whole defect.
        StashColumn(&'static str),
        /// A structural reconcile brings the pane back into the layout window.
        SurfaceColumn(&'static str),
        /// One pane-layout focus effect for the named document.
        FocusEffect(&'static str),
    }

    #[derive(Debug, PartialEq, Eq)]
    enum FocusOutcome {
        Applied(String),
        RefusedNotCoVisible(String),
        PaneNotFound,
    }

    struct World {
        /// The layout's file→pane assignment: a record, never re-derived.
        file_panes: Vec<(&'static str, &'static str)>,
        /// The window each pane was bound in. Records do not follow a move.
        recorded_window: BTreeMap<&'static str, &'static str>,
        /// Where each pane actually is right now.
        live_window: BTreeMap<&'static str, &'static str>,
        /// The tmux active pane — what the operator's focus mirrors onto.
        selected: Option<&'static str>,
        outcomes: Vec<FocusOutcome>,
        /// When true, the focus guard compares record against record (the
        /// pre-fix behavior) instead of record against live.
        record_only_guard: bool,
    }

    impl World {
        /// Two columns in the `agent-doc` window; `left` is the focus target.
        fn two_column_layout() -> Self {
            Self {
                file_panes: vec![("/tasks/left.md", "%76"), ("/tasks/right.md", "%75")],
                recorded_window: BTreeMap::from([("%76", LAYOUT_WINDOW), ("%75", LAYOUT_WINDOW)]),
                live_window: BTreeMap::from([("%76", LAYOUT_WINDOW), ("%75", LAYOUT_WINDOW)]),
                selected: Some("%75"),
                outcomes: Vec::new(),
                record_only_guard: false,
            }
        }

        fn pane_for(&self, document: &str) -> Option<&'static str> {
            self.file_panes
                .iter()
                .find(|(file, _)| *file == document)
                .map(|(_, pane)| *pane)
        }

        fn step(&mut self, action: Action) {
            match action {
                Action::StashColumn(document) => {
                    if let Some(pane) = self.pane_for(document) {
                        self.live_window.insert(pane, STASH_WINDOW);
                    }
                }
                Action::SurfaceColumn(document) => {
                    if let Some(pane) = self.pane_for(document) {
                        self.live_window.insert(pane, LAYOUT_WINDOW);
                    }
                }
                Action::FocusEffect(document) => {
                    let Some(pane) = self.pane_for(document) else {
                        self.outcomes.push(FocusOutcome::PaneNotFound);
                        return;
                    };
                    // The guard's observation side: the live window, or the
                    // bind-time record under the record-only mutation.
                    let observed = if self.record_only_guard {
                        self.recorded_window.get(pane).copied()
                    } else {
                        self.live_window.get(pane).copied()
                    };
                    if pane_window_binding_drifted(LAYOUT_WINDOW, observed) {
                        self.outcomes
                            .push(FocusOutcome::RefusedNotCoVisible(pane.to_string()));
                        return;
                    }
                    self.selected = Some(pane);
                    self.outcomes.push(FocusOutcome::Applied(pane.to_string()));
                }
            }
        }

        /// The operator-visible property: focus never lands on a pane that is
        /// not in the layout window.
        fn selected_pane_is_co_visible(&self) -> bool {
            self.selected
                .and_then(|pane| self.live_window.get(pane).copied())
                .is_none_or(|window| window == LAYOUT_WINDOW)
        }
    }

    #[test]
    fn a_stashed_column_never_receives_mirrored_focus() {
        let mut world = World::two_column_layout();
        world.step(Action::StashColumn("/tasks/left.md"));
        world.step(Action::FocusEffect("/tasks/left.md"));

        assert_eq!(
            world.outcomes,
            vec![FocusOutcome::RefusedNotCoVisible("%76".to_string())]
        );
        assert_eq!(
            world.selected,
            Some("%75"),
            "the visible active pane is left alone"
        );
        assert!(world.selected_pane_is_co_visible());
    }

    #[test]
    fn a_surfaced_column_focuses_on_the_next_attempt() {
        let mut world = World::two_column_layout();
        world.step(Action::StashColumn("/tasks/left.md"));
        world.step(Action::FocusEffect("/tasks/left.md"));
        // The structural reconcile the refusal does not short-circuit.
        world.step(Action::SurfaceColumn("/tasks/left.md"));
        world.step(Action::FocusEffect("/tasks/left.md"));

        assert_eq!(
            world.outcomes,
            vec![
                FocusOutcome::RefusedNotCoVisible("%76".to_string()),
                FocusOutcome::Applied("%76".to_string()),
            ]
        );
        assert_eq!(world.selected, Some("%76"));
        assert!(world.selected_pane_is_co_visible());
    }

    /// Sensitivity proof: with the pre-fix record-vs-record guard the stashed
    /// pane still reads as co-visible, focus is mirrored onto it, and the
    /// invariant goes red. Without this the passing test above would not prove
    /// the live comparison is what does the work.
    #[test]
    fn record_only_guard_mirrors_focus_onto_the_stashed_pane() {
        let mut world = World::two_column_layout();
        world.record_only_guard = true;
        world.step(Action::StashColumn("/tasks/left.md"));
        world.step(Action::FocusEffect("/tasks/left.md"));

        assert_eq!(
            world.outcomes,
            vec![FocusOutcome::Applied("%76".to_string())],
            "records agree, so a record-only guard cannot see the move"
        );
        assert!(
            !world.selected_pane_is_co_visible(),
            "the pre-fix guard is what put focus on a pane the operator cannot see"
        );
    }
}

/// Adversarial model for `#percellconverge` phase 3. The retained agent
/// transition owns only `exchange`; the editor changes `queue` on every tick.
/// The survival assertion is paired with an ownership-overclaim mutation proof
/// so the test demonstrates sensitivity to the exact ownership set.
#[cfg(test)]
mod owned_component_convergence_model {
    use std::collections::BTreeSet;

    use agent_doc_document::authority_hashes::{
        changed_component_names, owned_component_names_converged,
    };

    fn document(exchange: &str, queue: &str) -> String {
        format!(
            "<!-- agent:exchange -->\n{exchange}\n<!-- /agent:exchange -->\n\
             <!-- agent:queue -->\n{queue}\n<!-- /agent:queue -->\n"
        )
    }

    fn component_body<'a>(document: &'a str, name: &str) -> &'a str {
        agent_doc_element::element::parse(document)
            .unwrap()
            .into_iter()
            .find(|component| component.name == name)
            .unwrap()
            .content(document)
    }

    /// Project the agent target only for owned names, retaining the editor cut
    /// everywhere else. This is intentionally tiny and test-only: it makes an
    /// ownership over-claim observably destructive instead of merely blocked.
    fn project_owned_components(
        agent_target: &str,
        editor_cut: &str,
        owned: &BTreeSet<String>,
    ) -> String {
        let mut projected = editor_cut.to_string();
        for name in owned {
            let agent_components = agent_doc_element::element::parse(agent_target).unwrap();
            let Some(agent_component) = agent_components
                .into_iter()
                .find(|component| component.name == *name)
            else {
                continue;
            };
            let editor_components = agent_doc_element::element::parse(&projected).unwrap();
            let Some(editor_component) = editor_components
                .into_iter()
                .find(|component| component.name == *name)
            else {
                continue;
            };
            projected =
                editor_component.replace_content(&projected, agent_component.content(agent_target));
        }
        projected
    }

    fn run_ticks(owned: &BTreeSet<String>, ticks: usize) -> (String, String) {
        let baseline = document("prior response", "- queued work");
        let agent_target = document("new agent response", "- queued work");
        let mut disk = agent_target.clone();
        let mut typed = String::new();

        for tick in 0..ticks {
            typed.push(char::from(b'a' + (tick % 26) as u8));
            let authority = document(
                "new agent response",
                &format!("- queued work\n- operator keystrokes: {typed}"),
            );

            if owned_component_names_converged(&authority, &disk, owned).unwrap() {
                disk = project_owned_components(&agent_target, &authority, owned);
            }
        }

        assert_eq!(
            changed_component_names(&baseline, &agent_target).unwrap(),
            BTreeSet::from(["exchange".to_string()])
        );
        (disk, typed)
    }

    #[test]
    fn exchange_write_commits_while_every_operator_queue_keystroke_survives() {
        let owned = BTreeSet::from(["exchange".to_string()]);
        let (committed, typed) = run_ticks(&owned, 64);

        assert_eq!(
            component_body(&committed, "exchange").trim(),
            "new agent response"
        );
        assert!(
            component_body(&committed, "queue").contains(&typed),
            "all operator keystrokes must survive the scoped commit"
        );
    }

    #[test]
    fn ownership_overclaim_proves_the_survival_oracle_can_go_red() {
        let overclaimed = BTreeSet::from(["exchange".to_string(), "queue".to_string()]);
        let baseline = document("prior response", "- queued work");
        let agent_target = document("new agent response", "- queued work");
        let editor_cut = document(
            "new agent response",
            "- queued work\n- operator keystrokes: abcdef",
        );
        let destructive_projection =
            project_owned_components(&agent_target, &editor_cut, &overclaimed);

        assert!(
            !component_body(&destructive_projection, "queue").contains("abcdef"),
            "the mutation proof must show an over-claimed queue drops operator text"
        );
        assert_eq!(
            changed_component_names(&baseline, &agent_target).unwrap(),
            BTreeSet::from(["exchange".to_string()]),
            "the real ownership derivation must reject the over-claim"
        );
    }
}

/// `#jbcoldstartcrdtowner` reference model. Pane focus/layout sync is an
/// independent fast lane while a replacement controller fences a retained
/// editor lineage and lazily reprojects the retained canonical target.
mod cold_start_crdt_authority_model {
    use agent_doc_crdt_relay_io::{
        ColdStartReplicaUpdateDecision, decide_cold_start_replica_update,
    };

    const OBSERVED_PANE_SYNC_MS: u64 = 1_063;
    const PANE_SYNC_BUDGET_MS: u64 = 2_000;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Projection {
        DoubledRetained,
        Target,
    }

    #[derive(Clone, Copy)]
    enum Action {
        Restart,
        RetainedRegistration,
        RetainedIncrementalUpdate,
        CanonicalProjection,
        PaneSync,
    }

    const ACTIONS: [Action; 5] = [
        Action::Restart,
        Action::RetainedRegistration,
        Action::RetainedIncrementalUpdate,
        Action::CanonicalProjection,
        Action::PaneSync,
    ];

    #[derive(Clone)]
    struct World {
        registered: bool,
        canonical_projection_pending: bool,
        authority: Projection,
        retained_editor: Projection,
        stale_updates_quarantined: usize,
        semantic_recovery_successes: usize,
        pane_sync_ms: Option<u64>,
        disk_fallbacks: usize,
        force_disk_repairs: usize,
    }

    impl Default for World {
        fn default() -> Self {
            Self {
                registered: true,
                canonical_projection_pending: false,
                authority: Projection::Target,
                retained_editor: Projection::DoubledRetained,
                stale_updates_quarantined: 0,
                semantic_recovery_successes: 0,
                pane_sync_ms: None,
                disk_fallbacks: 0,
                force_disk_repairs: 0,
            }
        }
    }

    impl World {
        fn step(&mut self, action: Action) {
            match action {
                Action::Restart => {
                    self.registered = false;
                    self.canonical_projection_pending = true;
                    // The controller/Lazily key survives relay reconstruction;
                    // a disk-seeded transport replica is only a downstream
                    // consumer awaiting this retained target.
                    self.authority = Projection::Target;
                    self.semantic_recovery_successes = 0;
                }
                Action::RetainedRegistration => {
                    self.registered = true;
                    self.canonical_projection_pending = true;
                }
                Action::RetainedIncrementalUpdate => {
                    match decide_cold_start_replica_update(
                        self.registered,
                        self.authority == Projection::Target,
                        self.canonical_projection_pending,
                    ) {
                        ColdStartReplicaUpdateDecision::Relay => {
                            // Once full-state convergence established the shared
                            // lineage, an incremental frame advances that one
                            // projection; it never resurrects the retained 2× state.
                            debug_assert_eq!(self.authority, Projection::Target);
                        }
                        ColdStartReplicaUpdateDecision::ReprojectCanonical => {
                            self.registered = true;
                            self.authority = Projection::Target;
                            self.canonical_projection_pending = true;
                            self.stale_updates_quarantined += 1;
                        }
                    }
                }
                Action::CanonicalProjection => {
                    if self.canonical_projection_pending {
                        self.retained_editor = self.authority;
                        self.canonical_projection_pending = false;
                        self.semantic_recovery_successes = 1;
                    }
                }
                Action::PaneSync => {
                    self.pane_sync_ms = Some(OBSERVED_PANE_SYNC_MS);
                }
            }
            self.assert_invariants();
        }

        fn assert_invariants(&self) {
            assert_ne!(
                self.authority,
                Projection::DoubledRetained,
                "the retained 2× lineage must never become controller authority"
            );
            assert!(
                self.semantic_recovery_successes <= 1,
                "target convergence has exactly one semantic success receipt"
            );
            assert_eq!(self.disk_fallbacks, 0);
            assert_eq!(self.force_disk_repairs, 0);
            if let Some(elapsed_ms) = self.pane_sync_ms {
                assert!(
                    elapsed_ms < PANE_SYNC_BUDGET_MS,
                    "pane sync stays on the focus/layout fast lane"
                );
            }
        }
    }

    fn explore(world: World, depth: usize) {
        world.assert_invariants();
        if depth == 0 {
            return;
        }
        for action in ACTIONS {
            let mut next = world.clone();
            next.step(action);
            explore(next, depth - 1);
        }
    }

    #[test]
    fn restart_fences_doubled_lineage_and_recovers_exactly_once_off_the_pane_fast_lane() {
        let mut world = World::default();
        world.step(Action::Restart);
        world.step(Action::RetainedIncrementalUpdate);
        world.step(Action::RetainedIncrementalUpdate);
        world.step(Action::PaneSync);
        world.step(Action::CanonicalProjection);
        world.step(Action::CanonicalProjection);

        assert_eq!(world.authority, Projection::Target);
        assert_eq!(world.semantic_recovery_successes, 1);
        assert_eq!(world.pane_sync_ms, Some(OBSERVED_PANE_SYNC_MS));
        assert!(world.stale_updates_quarantined >= 1);
    }

    #[test]
    fn registration_arriving_first_cannot_promote_the_retained_lineage() {
        let mut world = World::default();
        world.step(Action::Restart);
        world.step(Action::RetainedRegistration);
        world.step(Action::RetainedIncrementalUpdate);
        assert_eq!(world.authority, Projection::Target);
        assert!(world.canonical_projection_pending);

        world.step(Action::CanonicalProjection);
        world.step(Action::RetainedIncrementalUpdate);
        assert_eq!(world.authority, Projection::Target);
        assert_eq!(world.semantic_recovery_successes, 1);
    }

    #[test]
    fn bounded_restart_orderings_preserve_single_authority_and_fast_pane_sync() {
        explore(World::default(), 7);
    }
}

/// Cross-root pane identity reference model. The sync effect owns exact
/// file-to-pane evidence for one desired generation; the observer must use
/// that evidence instead of re-inferring a nested document from a partial
/// project actor store.
mod pane_layout_effect_assignment_model {
    #[derive(Clone, Debug, PartialEq, Eq)]
    struct Assignment {
        generation: u64,
        file_panes: Vec<(&'static str, &'static str)>,
    }

    #[derive(Default)]
    struct World {
        desired_generation: u64,
        receipt: Option<Assignment>,
        visible_panes: Vec<&'static str>,
        selected_pane: Option<&'static str>,
    }

    impl World {
        fn publish(&mut self, generation: u64) {
            self.desired_generation = generation;
        }

        fn apply(
            &mut self,
            generation: u64,
            file_panes: Vec<(&'static str, &'static str)>,
            visible_panes: Vec<&'static str>,
        ) {
            self.receipt = Some(Assignment {
                generation,
                file_panes,
            });
            self.visible_panes = visible_panes;
        }

        fn observe(&self, documents: &[&str]) -> bool {
            let Some(receipt) = self
                .receipt
                .as_ref()
                .filter(|receipt| receipt.generation == self.desired_generation)
            else {
                return false;
            };
            let actual = self
                .visible_panes
                .iter()
                .map(|pane| {
                    receipt
                        .file_panes
                        .iter()
                        .find_map(|(file, assigned)| (assigned == pane).then_some(*file))
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>();
            actual == documents
        }

        fn focus(&mut self, file: &str) -> bool {
            let Some(receipt) = self
                .receipt
                .as_ref()
                .filter(|receipt| receipt.generation == self.desired_generation)
            else {
                return false;
            };
            let Some((_, pane)) = receipt
                .file_panes
                .iter()
                .find(|(assigned_file, _)| *assigned_file == file)
            else {
                return false;
            };
            if !self.visible_panes.contains(pane) {
                return false;
            }
            self.selected_pane = Some(*pane);
            true
        }
    }

    #[test]
    fn nested_actor_omission_does_not_stall_navigation_or_reuse_stale_assignment() {
        let mut world = World::default();
        world.publish(11);
        // Opening the nested document may briefly expose a provisioning pane,
        // but the effect's terminal assignment is the two-pane desired layout.
        world.apply(
            11,
            vec![
                ("tasks/primary.md", "%1"),
                ("nested/tasks/secondary.md", "%2"),
            ],
            vec!["%1", "%2"],
        );
        assert!(world.observe(&["tasks/primary.md", "nested/tasks/secondary.md"]));

        // A newer editor navigation invalidates the old assignment immediately.
        world.publish(12);
        assert!(!world.focus("tasks/next.md"));
        world.apply(
            12,
            vec![("tasks/primary.md", "%1"), ("tasks/next.md", "%3")],
            vec!["%1", "%3"],
        );
        assert!(world.observe(&["tasks/primary.md", "tasks/next.md"]));
        assert!(world.focus("tasks/next.md"));
        assert_eq!(world.selected_pane, Some("%3"));
    }
}

/// Installed-build handoff reference model. The controller process is the
/// lifetime of the reactive actor: a replacement rebuilds its Sources from the
/// durable projection, while write ordinals prevent observations from an older
/// lineage from settling a newer intent.
mod retained_write_generation_model {
    use agent_doc_state_backbone::retained_write::{
        RetainedIntentFacts, SettlementVerdict, durable_exact_observations, settlement_verdict,
    };
    use agent_doc_state_backbone::{
        DocumentAuthority, DocumentStateProjection, DocumentWriteDeferredReason,
        DocumentWriteSource, StateFact,
    };

    enum Action {
        ObserveEditor {
            epoch: u64,
            hash: &'static str,
        },
        ObserveDisk {
            epoch: u64,
            hash: &'static str,
        },
        Defer {
            intent_id: &'static str,
            target: &'static str,
        },
        ReplaceController,
    }

    struct World {
        projection: DocumentStateProjection,
        actor_generation: u64,
        settled: bool,
    }

    impl Default for World {
        fn default() -> Self {
            Self {
                projection: DocumentStateProjection::new("generation-rebuild-document"),
                actor_generation: 1,
                settled: false,
            }
        }
    }

    impl World {
        fn step(&mut self, action: Action) {
            match action {
                Action::ObserveEditor { epoch, hash } => {
                    self.observe(DocumentAuthority::EditorRelay, epoch, hash);
                }
                Action::ObserveDisk { epoch, hash } => {
                    self.observe(DocumentAuthority::DiskReplica, epoch, hash);
                }
                Action::Defer { intent_id, target } => {
                    self.projection
                        .apply_fact(&StateFact::DocumentWriteDeferred {
                            document_hash: self.projection.document_hash.clone(),
                            intent_id: intent_id.to_string(),
                            expected_hash: "before".to_string(),
                            expected_content: None,
                            target_hash: target.to_string(),
                            target_content: format!("content-{target}"),
                            source: DocumentWriteSource::PendingWrite,
                            reason: DocumentWriteDeferredReason::EditorProjectionPending,
                        });
                }
                Action::ReplaceController => {
                    self.actor_generation += 1;
                    let Some(pending) = self.projection.document.pending_write.as_ref() else {
                        return;
                    };
                    let facts = RetainedIntentFacts {
                        intent_id: pending.intent_id.clone(),
                        target_hash: pending.target_hash.clone(),
                        reason: pending.reason.clone(),
                        source: pending.source.clone(),
                        superseding_stage: None,
                        carries_response_payload: false,
                        carries_content_delta: true,
                    };
                    let (editor, disk) = durable_exact_observations(&self.projection.document);
                    self.settled = matches!(
                        settlement_verdict(Some(&facts), editor.as_ref(), disk.as_ref()),
                        SettlementVerdict::Satisfied { .. }
                    );
                }
            }
        }

        fn observe(&mut self, authority: DocumentAuthority, epoch: u64, hash: &str) {
            self.projection
                .apply_fact(&StateFact::DocumentAuthorityObserved {
                    document_hash: self.projection.document_hash.clone(),
                    authority,
                    authority_epoch: epoch,
                    source: "simworld".to_string(),
                    reason: "generation_rebuild".to_string(),
                    content_hash: Some(hash.to_string()),
                    editor_id: None,
                });
        }
    }

    #[test]
    fn replacement_actor_settles_exact_durable_planes_without_a_retry() {
        let mut world = World::default();
        world.step(Action::Defer {
            intent_id: "intent-a",
            target: "target-a",
        });
        world.step(Action::ObserveEditor {
            epoch: 10,
            hash: "target-a",
        });
        world.step(Action::ObserveDisk {
            epoch: 11,
            hash: "target-a",
        });
        world.step(Action::ReplaceController);

        assert_eq!(world.actor_generation, 2);
        assert!(world.settled);
    }

    #[test]
    fn replacement_actor_rejects_matching_planes_from_before_the_intent() {
        let mut world = World::default();
        world.step(Action::ObserveEditor {
            epoch: 10,
            hash: "target-b",
        });
        world.step(Action::ObserveDisk {
            epoch: 11,
            hash: "target-b",
        });
        world.step(Action::Defer {
            intent_id: "intent-b",
            target: "target-b",
        });
        world.step(Action::ReplaceController);

        assert_eq!(world.actor_generation, 2);
        assert!(!world.settled);
    }
}

/// `#orphandrain` reference model: detached route dispatch and durable backoff
/// remain safe across controller contention and restart.
mod orphan_drain_model {
    use agent_doc_controller::orphan_drain::{
        DEFAULT_MIN_DISPATCH_INTERVAL_SECS, OrphanDrainDecision, OrphanDrainObservation,
        OrphanDrainQueueControl, orphan_drain_decision,
    };
    use agent_doc_route_io::pane_resolution::{
        BackgroundExistingPaneDecision, background_existing_pane_decision,
    };

    #[derive(Clone, Copy, Debug)]
    enum Action {
        ControllerATick,
        ControllerBTick,
        RestartController,
        Advance89Seconds,
        AdvanceOneSecond,
        RouteSettles,
        PauseQueue,
        CloseOwnerPane,
        ReuseOwnerPaneForOtherDocument,
    }

    const ACTIONS: [Action; 6] = [
        Action::ControllerATick,
        Action::ControllerBTick,
        Action::RestartController,
        Action::Advance89Seconds,
        Action::AdvanceOneSecond,
        Action::RouteSettles,
    ];

    #[derive(Clone, Debug)]
    struct World {
        now: u64,
        durable_last_dispatch: Option<u64>,
        controller_generation: u64,
        route_in_flight: bool,
        controller_event_loop_blocked: bool,
        dispatches: usize,
        queue_control: OrphanDrainQueueControl,
        owner_pane_alive: bool,
        owner_pane_runs_other_document: bool,
        replacement_panes_created: usize,
        operator_focus_stolen: bool,
        blocked_background_routes: usize,
    }

    impl Default for World {
        fn default() -> Self {
            Self {
                now: 0,
                durable_last_dispatch: None,
                controller_generation: 0,
                route_in_flight: false,
                controller_event_loop_blocked: false,
                dispatches: 0,
                queue_control: OrphanDrainQueueControl::Runnable,
                owner_pane_alive: true,
                owner_pane_runs_other_document: false,
                replacement_panes_created: 0,
                operator_focus_stolen: false,
                blocked_background_routes: 0,
            }
        }
    }

    impl World {
        fn controller_tick(&mut self) {
            let observation = OrphanDrainObservation {
                queue_active: self.queue_control.allows_unattended_drain(),
                has_drainable_head: true,
                supervisor_alive: false,
                loop_owns_drain: false,
                // The route can still be starting before actor state changes to
                // busy; durable time, not a transient pane bit, is the fence.
                pane_busy: false,
                secs_since_last_dispatch: self
                    .durable_last_dispatch
                    .map(|last| self.now.saturating_sub(last)),
            };
            if orphan_drain_decision(observation, DEFAULT_MIN_DISPATCH_INTERVAL_SECS)
                != OrphanDrainDecision::Dispatch
            {
                return;
            }

            let pane_decision = background_existing_pane_decision(
                Some("%owner"),
                self.owner_pane_alive,
                self.owner_pane_runs_other_document,
                Some("%owner"),
                Some("%owner"),
                Some("%owner"),
            );
            if pane_decision != BackgroundExistingPaneDecision::UseExistingPane {
                self.blocked_background_routes += 1;
                return;
            }

            // Simulate the old unrestricted route semantics if the pure guard
            // ever incorrectly admits a dead or foreign pane: it would search
            // or provision and surface the result. The invariant below makes
            // that policy regression fail immediately.
            if !self.owner_pane_alive || self.owner_pane_runs_other_document {
                self.replacement_panes_created += usize::from(!self.owner_pane_alive);
                self.operator_focus_stolen = true;
            }

            // Model the single-statement SQLite conditional upsert. Both
            // controllers share this durable cell; only the first contender at
            // an eligible instant can advance it.
            let eligible = self.durable_last_dispatch.is_none_or(|last| {
                self.now.saturating_sub(last) >= DEFAULT_MIN_DISPATCH_INTERVAL_SECS
            });
            if eligible {
                if let Some(last) = self.durable_last_dispatch {
                    assert!(
                        self.now.saturating_sub(last) >= DEFAULT_MIN_DISPATCH_INTERVAL_SECS,
                        "orphan drain dispatched inside the durable backoff window"
                    );
                }
                self.durable_last_dispatch = Some(self.now);
                self.route_in_flight = true;
                self.dispatches += 1;
                // External route children may call back only after this tick;
                // the controller event loop itself is never synchronously held.
                self.controller_event_loop_blocked = false;
            }
        }

        fn step(&mut self, action: Action) {
            match action {
                Action::ControllerATick | Action::ControllerBTick => self.controller_tick(),
                Action::RestartController => {
                    self.controller_generation += 1;
                    self.controller_event_loop_blocked = false;
                }
                Action::Advance89Seconds => self.now += 89,
                Action::AdvanceOneSecond => self.now += 1,
                Action::RouteSettles => self.route_in_flight = false,
                Action::PauseQueue => self.queue_control = OrphanDrainQueueControl::Paused,
                Action::CloseOwnerPane => self.owner_pane_alive = false,
                Action::ReuseOwnerPaneForOtherDocument => {
                    self.owner_pane_alive = true;
                    self.owner_pane_runs_other_document = true;
                }
            }
            assert!(
                !self.controller_event_loop_blocked,
                "detached orphan drain must never self-block the controller"
            );
            assert_eq!(
                self.replacement_panes_created, 0,
                "background orphan recovery must never create a replacement pane"
            );
            assert!(
                !self.operator_focus_stolen,
                "background orphan recovery must preserve operator tmux focus"
            );
        }
    }

    fn explore(world: World, depth: usize) {
        if depth == 0 {
            return;
        }
        for action in ACTIONS {
            let mut next = world.clone();
            next.step(action);
            explore(next, depth - 1);
        }
    }

    #[test]
    fn contention_and_restart_do_not_recreate_the_dispatch_storm() {
        let mut world = World::default();
        world.step(Action::ControllerATick);
        world.step(Action::ControllerBTick);
        world.step(Action::RestartController);
        world.step(Action::ControllerATick);
        assert_eq!(world.dispatches, 1);
        assert!(world.route_in_flight);

        world.step(Action::Advance89Seconds);
        world.step(Action::ControllerBTick);
        assert_eq!(world.dispatches, 1);

        world.step(Action::AdvanceOneSecond);
        world.step(Action::ControllerATick);
        assert_eq!(world.dispatches, 2);
    }

    #[test]
    fn bounded_interleavings_preserve_detachment_and_backoff() {
        explore(World::default(), 7);
    }

    #[test]
    fn paused_stale_head_and_closed_or_reused_panes_never_reopen_or_steal_focus() {
        let mut paused = World::default();
        paused.step(Action::PauseQueue);
        paused.step(Action::ControllerATick);
        paused.step(Action::Advance89Seconds);
        paused.step(Action::AdvanceOneSecond);
        paused.step(Action::ControllerBTick);
        assert_eq!(paused.dispatches, 0, "a paused retained head is not active");

        let mut closed = World::default();
        closed.step(Action::CloseOwnerPane);
        closed.step(Action::ControllerATick);
        closed.step(Action::Advance89Seconds);
        closed.step(Action::AdvanceOneSecond);
        closed.step(Action::ControllerBTick);
        assert_eq!(closed.dispatches, 0);
        assert_eq!(closed.blocked_background_routes, 2);

        let mut reused = World::default();
        reused.step(Action::ReuseOwnerPaneForOtherDocument);
        reused.step(Action::ControllerATick);
        reused.step(Action::Advance89Seconds);
        reused.step(Action::AdvanceOneSecond);
        reused.step(Action::ControllerBTick);
        assert_eq!(reused.dispatches, 0);
        assert_eq!(reused.blocked_background_routes, 2);
    }
}

/// `#qactsync` model: marker gestures and canonical frontmatter controls
/// converge bidirectionally, and every settled state is a fixed point.
mod queue_activation_binding_model {
    use agent_doc_frontmatter::frontmatter;
    use agent_doc_queue::control_binding::{
        converge_queue_control_binding_content, explicit_queue_go_mode, explicit_queue_start_mode,
        explicit_queue_stop_mode,
    };

    #[derive(Clone, Copy, Debug)]
    enum Action {
        MarkerStarts,
        MarkerGoes,
        MarkerStops,
        FrontmatterStarts,
        FrontmatterStops,
        ConflictingTwoSidedEdit,
    }

    const ACTIONS: [Action; 6] = [
        Action::MarkerStarts,
        Action::MarkerGoes,
        Action::MarkerStops,
        Action::FrontmatterStarts,
        Action::FrontmatterStops,
        Action::ConflictingTwoSidedEdit,
    ];

    #[derive(Clone, Debug)]
    struct World {
        content: String,
    }

    impl Default for World {
        fn default() -> Self {
            Self {
                content: stopped_document(),
            }
        }
    }

    impl World {
        fn step(&mut self, action: Action) {
            let (snapshot, edited) = match action {
                Action::MarkerStarts => {
                    let snapshot = self.content.clone();
                    let edited = set_marker_control(&snapshot, Some("start"));
                    (snapshot, edited)
                }
                Action::MarkerGoes => {
                    let snapshot = self.content.clone();
                    let edited = set_marker_control(&snapshot, Some("go"));
                    (snapshot, edited)
                }
                Action::MarkerStops => {
                    let snapshot = self.content.clone();
                    let edited = set_marker_control(&snapshot, None);
                    (snapshot, edited)
                }
                Action::FrontmatterStarts => {
                    let snapshot = self.content.clone();
                    let edited = frontmatter::merge_queue_control(&snapshot, "start").unwrap();
                    (snapshot, edited)
                }
                Action::FrontmatterStops => {
                    let snapshot = self.content.clone();
                    let edited = frontmatter::merge_queue_control(&snapshot, "stop").unwrap();
                    (snapshot, edited)
                }
                Action::ConflictingTwoSidedEdit => {
                    let snapshot = started_document();
                    let marker_edited = set_marker_control(&snapshot, Some("go"));
                    let edited = frontmatter::merge_queue_control(&marker_edited, "stop").unwrap();
                    (snapshot, edited)
                }
            };

            let (settled, _) =
                converge_queue_control_binding_content(&edited, Some(&snapshot)).unwrap();
            assert_synced(&settled);

            let (fixed_point, changed_again) =
                converge_queue_control_binding_content(&settled, Some(&snapshot)).unwrap();
            assert!(
                !changed_again,
                "settled queue control must be a fixed point"
            );
            assert_eq!(fixed_point, settled);
            self.content = settled;
        }
    }

    fn stopped_document() -> String {
        concat!(
            "---\n",
            "agent_doc_session: sample\n",
            "queue: stop\n",
            "---\n\n",
            "<!-- agent:queue -->\n",
            "- do [#sample]\n",
            "<!-- /agent:queue -->\n",
        )
        .to_string()
    }

    fn started_document() -> String {
        stopped_document()
            .replacen("queue: stop", "queue: start", 1)
            .replacen("<!-- agent:queue -->", "<!-- agent:queue start -->", 1)
    }

    fn set_marker_control(content: &str, control: Option<&str>) -> String {
        let components = agent_doc_element::element::parse(content).unwrap();
        let queue = components
            .iter()
            .find(|component| component.name == "queue")
            .unwrap();
        let raw_tag = &content[queue.open_start..queue.open_end];
        let new_tag = agent_doc_queue::document_queue::set_control_in_tag(raw_tag, control);
        let mut updated = String::with_capacity(content.len());
        updated.push_str(&content[..queue.open_start]);
        updated.push_str(&new_tag);
        updated.push_str(&content[queue.open_end..]);
        updated
    }

    fn assert_synced(content: &str) {
        let (fm, _) = frontmatter::parse(content).unwrap();
        let components = agent_doc_element::element::parse(content).unwrap();
        let queue = components
            .iter()
            .find(|component| component.name == "queue")
            .unwrap();
        let controls = [
            explicit_queue_start_mode(&queue.attrs, fm.queue.as_deref()),
            explicit_queue_go_mode(&queue.attrs, fm.queue.as_deref()),
            explicit_queue_stop_mode(&queue.attrs, fm.queue.as_deref()),
        ];
        assert_eq!(
            controls.into_iter().filter(|active| *active).count(),
            1,
            "queue marker and frontmatter must resolve to one control: {content}"
        );
    }

    fn explore(world: World, depth: usize) {
        if depth == 0 {
            return;
        }
        for action in ACTIONS {
            let mut next = world.clone();
            next.step(action);
            explore(next, depth - 1);
        }
    }

    #[test]
    fn both_edit_directions_and_conflicts_converge() {
        let mut world = World::default();
        world.step(Action::MarkerStarts);
        world.step(Action::FrontmatterStops);
        world.step(Action::MarkerGoes);
        world.step(Action::MarkerStops);
        world.step(Action::FrontmatterStarts);
        world.step(Action::ConflictingTwoSidedEdit);
    }

    #[test]
    fn bounded_sequences_never_reintroduce_activation_churn() {
        explore(World::default(), 5);
    }
}

/// Retained-response retry model: the original editor cut remains the merge
/// base while response delivery, editor prompt edits, replica acceptance, and
/// controller restart can interleave. A durable queue id is a singleton across
/// every retry after the operator has cleaned its replayed copies.
mod retained_queue_retry_model {
    use agent_doc_merge::crdt::{CrdtDoc, merge_by_component};

    const ITEM: &str = "- do [#autoinstalldeferstale] once";

    #[derive(Clone, Copy, Debug)]
    enum Action {
        Retry,
        EditorAddsPrompt,
        EditorCleansQueue,
        ReplicaAcceptsTarget,
        ControllerRestarts,
    }

    const ACTIONS: [Action; 5] = [
        Action::Retry,
        Action::EditorAddsPrompt,
        Action::EditorCleansQueue,
        Action::ReplicaAcceptsTarget,
        Action::ControllerRestarts,
    ];

    #[derive(Clone, Debug)]
    struct World {
        base_state: Vec<u8>,
        retained_target: String,
        editor_cut: String,
        retries: usize,
    }

    impl World {
        fn new() -> Self {
            let base = concat!(
                "<!-- agent:exchange -->\n",
                "❯ Apply the change.\n",
                "<!-- agent:boundary:base -->\n",
                "<!-- /agent:exchange -->\n",
                "<!-- agent:queue -->\n",
                "- do [#autoinstalldeferstale] once\n",
                "<!-- /agent:queue -->\n",
            );
            let retained_target = base
                .replace(
                    "<!-- agent:boundary:base -->",
                    "### Re: retained — gpt-5\n\nRetained response.\n<!-- agent:boundary:response -->",
                )
                .replace(
                    "- do [#autoinstalldeferstale] once\n",
                    concat!(
                        "- do [#autoinstalldeferstale] once\n",
                        "- do [#autoinstalldeferstale] once\n",
                        "- do [#autoinstalldeferstale] once\n",
                    ),
                );
            Self {
                base_state: CrdtDoc::from_text(base).encode_state(),
                retained_target,
                editor_cut: base.to_string(),
                retries: 0,
            }
        }

        fn step(&mut self, action: Action) {
            match action {
                Action::Retry => {
                    let operator_item_count = self.editor_cut.matches(ITEM).count();
                    self.retained_target = merge_by_component(
                        Some(&self.base_state),
                        &self.retained_target,
                        &self.editor_cut,
                    )
                    .expect("retained retry merge");
                    self.retries += 1;
                    if operator_item_count <= 1 {
                        assert_eq!(
                            self.retained_target.matches(ITEM).count(),
                            operator_item_count,
                            "retained retry overrode the operator-cleaned queue multiplicity"
                        );
                    }
                    assert_eq!(
                        self.retained_target.matches("agent:boundary:").count(),
                        1,
                        "retained retry duplicated the response boundary"
                    );
                }
                Action::EditorAddsPrompt => {
                    if !self.editor_cut.contains("❯ Prompt during delivery.") {
                        self.editor_cut = self.editor_cut.replace(
                            "❯ Apply the change.\n",
                            "❯ Apply the change.\n❯ Prompt during delivery.\n",
                        );
                    }
                }
                Action::EditorCleansQueue => {
                    let mut seen = false;
                    self.editor_cut = self
                        .editor_cut
                        .split_inclusive('\n')
                        .filter(|line| {
                            if line.trim_end() != ITEM {
                                return true;
                            }
                            let keep = !seen;
                            seen = true;
                            keep
                        })
                        .collect();
                }
                Action::ReplicaAcceptsTarget => {
                    self.editor_cut = self.retained_target.clone();
                }
                Action::ControllerRestarts => {
                    // The retained target and original content-bearing base are
                    // durable. Restart changes no merge inputs.
                }
            }
        }
    }

    fn explore(world: World, depth: usize) {
        if depth == 0 {
            return;
        }
        for action in ACTIONS {
            let mut next = world.clone();
            next.step(action);
            explore(next, depth - 1);
        }
    }

    #[test]
    fn bounded_retry_interleavings_keep_tagged_queue_item_singleton() {
        let mut direct = World::new();
        direct.step(Action::Retry);
        direct.step(Action::ControllerRestarts);
        direct.step(Action::EditorAddsPrompt);
        direct.step(Action::Retry);
        assert_eq!(direct.retries, 2);

        explore(World::new(), 5);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CyclePhase {
    Idle,
    PreflightStarted,
    ResponseCaptured,
    WriteApplied,
    Interrupted(FaultPoint),
    Committed,
}

/// Focused reference model for the capture -> compact -> commit closeout path.
/// The state space distinguishes an exact compact archive from a merely
/// response-shaped, unrelated archive so the fail-closed boundary is exercised
/// independently of filesystem and IDE adapters.
mod capture_compact_closeout_model {
    use agent_doc_workflow::capture::{
        CaptureCloseoutMaterializationBasis, CaptureCloseoutMaterializationDecision,
        CaptureCloseoutMaterializationEvidence, decide_capture_closeout_materialization,
    };

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    enum ArchiveReference {
        #[default]
        None,
        Unrelated,
        Exact,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Action {
        Capture,
        ExposeCommitSurface,
        MaterializeInline,
        CompactIntoExactArchive,
        ReferenceUnrelatedArchive,
        Commit,
    }

    const ACTIONS: [Action; 6] = [
        Action::Capture,
        Action::ExposeCommitSurface,
        Action::MaterializeInline,
        Action::CompactIntoExactArchive,
        Action::ReferenceUnrelatedArchive,
        Action::Commit,
    ];

    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    struct World {
        active_capture: bool,
        capture_terminal: bool,
        commit_surface_available: bool,
        response_in_commit_surface: bool,
        archive_reference: ArchiveReference,
        unsafe_closeouts: usize,
    }

    #[derive(Debug, Default)]
    struct Coverage {
        exact_archive_allowed: bool,
        missing_response_blocked: bool,
        unrelated_archive_blocked: bool,
    }

    impl World {
        fn evidence(&self) -> CaptureCloseoutMaterializationEvidence {
            CaptureCloseoutMaterializationEvidence {
                active_capture: self.active_capture,
                capture_terminal: self.capture_terminal,
                commit_surface_available: self.commit_surface_available,
                response_in_commit_surface: self.response_in_commit_surface,
                response_in_referenced_compact_archive: matches!(
                    self.archive_reference,
                    ArchiveReference::Exact
                ),
            }
        }

        fn step(&mut self, action: Action, coverage: &mut Coverage) {
            match action {
                Action::Capture => {
                    self.active_capture = true;
                    self.capture_terminal = false;
                    self.commit_surface_available = false;
                    self.response_in_commit_surface = false;
                    self.archive_reference = ArchiveReference::None;
                }
                Action::ExposeCommitSurface => self.commit_surface_available = true,
                Action::MaterializeInline => {
                    self.commit_surface_available = true;
                    self.response_in_commit_surface = true;
                }
                Action::CompactIntoExactArchive => {
                    self.commit_surface_available = true;
                    self.response_in_commit_surface = false;
                    self.archive_reference = ArchiveReference::Exact;
                }
                Action::ReferenceUnrelatedArchive => {
                    self.commit_surface_available = true;
                    self.response_in_commit_surface = false;
                    self.archive_reference = ArchiveReference::Unrelated;
                }
                Action::Commit => {
                    let evidence = self.evidence();
                    match decide_capture_closeout_materialization(evidence) {
                        CaptureCloseoutMaterializationDecision::Allow(basis) => {
                            if evidence.active_capture
                                && !evidence.capture_terminal
                                && evidence.commit_surface_available
                                && !evidence.response_in_commit_surface
                                && !evidence.response_in_referenced_compact_archive
                            {
                                self.unsafe_closeouts += 1;
                            }
                            if basis
                                == CaptureCloseoutMaterializationBasis::ReferencedCompactArchive
                            {
                                coverage.exact_archive_allowed = true;
                            }
                            self.capture_terminal = true;
                        }
                        CaptureCloseoutMaterializationDecision::BlockMissingResponse => {
                            coverage.missing_response_blocked = true;
                            if self.archive_reference == ArchiveReference::Unrelated {
                                coverage.unrelated_archive_blocked = true;
                            }
                        }
                    }
                }
            }
        }
    }

    fn explore(world: World, depth: usize, coverage: &mut Coverage) {
        assert_eq!(
            world.unsafe_closeouts, 0,
            "closeout retired an open capture without exact response materialization: {world:?}"
        );
        if depth == 0 {
            return;
        }
        for action in ACTIONS {
            let mut next = world.clone();
            next.step(action, coverage);
            explore(next, depth - 1, coverage);
        }
    }

    #[test]
    fn exact_compact_archive_closes_while_unrelated_archive_blocks() {
        let mut coverage = Coverage::default();
        let mut exact = World::default();
        exact.step(Action::Capture, &mut coverage);
        exact.step(Action::CompactIntoExactArchive, &mut coverage);
        exact.step(Action::Commit, &mut coverage);
        assert!(exact.capture_terminal);
        assert!(coverage.exact_archive_allowed);

        let mut unrelated = World::default();
        unrelated.step(Action::Capture, &mut coverage);
        unrelated.step(Action::ReferenceUnrelatedArchive, &mut coverage);
        unrelated.step(Action::Commit, &mut coverage);
        assert!(!unrelated.capture_terminal);
        assert!(coverage.unrelated_archive_blocked);
    }

    #[test]
    fn exhaustive_capture_compact_closeout_schedules_preserve_materialization() {
        let mut coverage = Coverage::default();
        explore(World::default(), 6, &mut coverage);
        assert!(coverage.exact_archive_allowed);
        assert!(coverage.missing_response_blocked);
        assert!(coverage.unrelated_archive_blocked);
    }
}

/// Exhaustive executable reference model for the lineage fence. This is kept
/// independent of transport timing so every reachable ordering of replacement,
/// stale replay, restart, current delivery, and commit is checked cheaply.
mod crdt_lineage_fence_model {
    use std::collections::BTreeSet;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Action {
        OperatorDelete,
        CaptureAgentIntent,
        CaptureTrackedMutation,
        CaptureMalformedAgentTarget,
        ReplaceAndRebase,
        DeliverStale,
        DeliverCurrent,
        RestartMatchingProjection,
        RestartMismatchedProjection,
        RequestEditorSave,
        EditorNativeSave,
        OperatorAdvanceAfterSaveRequest,
        CrashDropsCleanQueue,
        RecoverDurableQueue,
        DeliverMalformedAgentTarget,
        Commit,
    }

    const ACTIONS: [Action; 16] = [
        Action::OperatorDelete,
        Action::CaptureAgentIntent,
        Action::CaptureTrackedMutation,
        Action::CaptureMalformedAgentTarget,
        Action::ReplaceAndRebase,
        Action::DeliverStale,
        Action::DeliverCurrent,
        Action::RestartMatchingProjection,
        Action::RestartMismatchedProjection,
        Action::RequestEditorSave,
        Action::EditorNativeSave,
        Action::OperatorAdvanceAfterSaveRequest,
        Action::CrashDropsCleanQueue,
        Action::RecoverDurableQueue,
        Action::DeliverMalformedAgentTarget,
        Action::Commit,
    ];

    #[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
    struct World {
        lineage: u8,
        queue_visible: bool,
        durable_queue_visible: bool,
        queue_tombstone: bool,
        clean_queue_replayed: bool,
        pending_agent_intent: bool,
        agent_intent_applied: bool,
        pending_tracked_mutation: bool,
        tracked_mutation_applied: bool,
        malformed_agent_target_pending: bool,
        malformed_agent_target_rejected: bool,
        stale_frame_pending: bool,
        current_frame_pending: bool,
        ack_cursor: u8,
        disk_has_agent_intent: bool,
        editor_save_requested: bool,
        committed: bool,
        corrupted: bool,
    }

    impl World {
        fn initial() -> Self {
            Self {
                queue_visible: true,
                durable_queue_visible: true,
                ..Self::default()
            }
        }

        fn step(&mut self, action: Action) {
            match action {
                Action::OperatorDelete if self.queue_visible => {
                    self.queue_visible = false;
                    self.durable_queue_visible = false;
                    self.queue_tombstone = true;
                    self.stale_frame_pending = true;
                }
                Action::CaptureAgentIntent if !self.committed => {
                    self.pending_agent_intent = true;
                    if self.lineage > 0 && !self.agent_intent_applied {
                        self.current_frame_pending = true;
                    }
                }
                Action::CaptureTrackedMutation if self.pending_agent_intent && !self.committed => {
                    // finalize owns one response + tracked-work envelope. Once
                    // requested, the bookkeeping half remains a commit
                    // precondition across ACK timeout and reconnect.
                    self.pending_tracked_mutation = true;
                    if self.lineage > 0 && !self.tracked_mutation_applied {
                        self.current_frame_pending = true;
                    }
                }
                Action::CaptureMalformedAgentTarget => {
                    self.malformed_agent_target_pending = true;
                }
                Action::ReplaceAndRebase if self.queue_tombstone && self.lineage == 0 => {
                    self.lineage = 1;
                    self.agent_intent_applied = self.pending_agent_intent;
                    self.tracked_mutation_applied = self.pending_tracked_mutation;
                    self.current_frame_pending =
                        self.pending_agent_intent || self.pending_tracked_mutation;
                }
                Action::DeliverStale if self.stale_frame_pending && self.lineage > 0 => {
                    // Production outcome: StaleLineage/LegacyQuarantined. The
                    // frame is terminally consumed without touching content.
                    self.stale_frame_pending = false;
                    self.ack_cursor = self.ack_cursor.saturating_add(1);
                }
                Action::DeliverCurrent if self.current_frame_pending => {
                    self.current_frame_pending = false;
                    self.agent_intent_applied = true;
                    self.tracked_mutation_applied = self.pending_tracked_mutation;
                    self.ack_cursor = self.ack_cursor.saturating_add(1);
                }
                Action::RestartMatchingProjection => {
                    // Projection hash + lineage metadata match: preserve epoch.
                }
                Action::RestartMismatchedProjection if self.lineage > 0 => {
                    // Fail closed: mint another lineage. Pending old frames can
                    // only be quarantined; durable agent intent remains journaled.
                    self.lineage = self.lineage.saturating_add(1);
                    if self.pending_agent_intent && !self.agent_intent_applied {
                        self.current_frame_pending = true;
                    }
                    if self.pending_tracked_mutation && !self.tracked_mutation_applied {
                        self.current_frame_pending = true;
                    }
                }
                Action::RequestEditorSave
                    if self.pending_agent_intent
                        && self.agent_intent_applied
                        && !self.current_frame_pending
                        && !self.disk_has_agent_intent =>
                {
                    self.editor_save_requested = true;
                }
                Action::EditorNativeSave
                    if self.editor_save_requested
                        && self.agent_intent_applied
                        && !self.current_frame_pending =>
                {
                    self.disk_has_agent_intent = true;
                    self.editor_save_requested = false;
                }
                Action::OperatorAdvanceAfterSaveRequest
                    if self.editor_save_requested && self.pending_agent_intent =>
                {
                    // A newer operator cut invalidates the old save proof. The
                    // durable agent intent must rebase and request another save.
                    self.agent_intent_applied = false;
                    self.current_frame_pending = true;
                    self.editor_save_requested = false;
                }
                Action::CrashDropsCleanQueue if self.queue_visible && !self.queue_tombstone => {
                    self.queue_visible = false;
                }
                Action::RecoverDurableQueue
                    if self.durable_queue_visible
                        && !self.queue_tombstone
                        && !self.queue_visible =>
                {
                    self.queue_visible = true;
                    self.clean_queue_replayed = true;
                }
                Action::DeliverMalformedAgentTarget if self.malformed_agent_target_pending => {
                    // The canonical/deferred write validity fence consumes the
                    // attempt without changing either Lazily or editor authority.
                    self.malformed_agent_target_pending = false;
                    self.malformed_agent_target_rejected = true;
                }
                Action::Commit
                    if self.pending_agent_intent
                        && self.agent_intent_applied
                        && (!self.pending_tracked_mutation || self.tracked_mutation_applied)
                        && !self.current_frame_pending
                        && self.disk_has_agent_intent =>
                {
                    self.committed = true;
                    self.pending_agent_intent = false;
                }
                _ => {}
            }
        }

        fn assert_invariants(&self) {
            assert!(
                !self.corrupted,
                "stale replay corrupted the canonical: {self:?}"
            );
            assert!(
                !self.queue_tombstone || !self.queue_visible,
                "deleted queue item resurrected: {self:?}"
            );
            assert!(
                !self.queue_tombstone || !self.durable_queue_visible,
                "operator deletion remained replayable in the journal: {self:?}"
            );
            assert!(
                !self.committed || (self.agent_intent_applied && self.disk_has_agent_intent),
                "commit preceded the exact native editor save: {self:?}"
            );
            assert!(
                !self.committed || !self.pending_tracked_mutation || self.tracked_mutation_applied,
                "response committed without its tracked-work mutation: {self:?}"
            );
            if self.pending_agent_intent && self.lineage > 0 {
                assert!(
                    self.agent_intent_applied || self.current_frame_pending,
                    "replacement lost the durable pending agent change: {self:?}"
                );
            }
        }
    }

    fn explore(world: World, depth: usize, visited: &mut BTreeSet<World>) {
        world.assert_invariants();
        if depth == 0 || !visited.insert(world.clone()) {
            return;
        }
        for action in ACTIONS {
            let mut next = world.clone();
            next.step(action);
            explore(next, depth - 1, visited);
        }
    }

    #[test]
    fn exhaustive_lineage_recovery_interleavings_preserve_monotonic_progress() {
        let mut visited = BTreeSet::new();
        explore(World::initial(), 18, &mut visited);
        assert!(visited.iter().any(|world| world.committed));
        assert!(visited.iter().any(|world| world.ack_cursor >= 2));
        assert!(visited.iter().any(|world| world.lineage >= 2));
        assert!(visited.iter().any(|world| world.editor_save_requested));
        assert!(visited.iter().any(|world| world.clean_queue_replayed));
        assert!(visited.iter().any(|world| world.tracked_mutation_applied));
        assert!(
            visited
                .iter()
                .any(|world| world.malformed_agent_target_rejected)
        );
    }
}

/// Formal transition model for one visible editor refreshing its native CRDT
/// replica while old direct and durable frames are still in flight. A refresh
/// generation is not a collaborative head: registration atomically retires the
/// prior generation and rotates the durable-frame lineage.
mod logical_editor_replica_refresh_model {
    use std::collections::BTreeSet;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Action {
        RegisterReplacement,
        LateOldDirectFrame,
        LateOldDurableFrame,
        CurrentDirectFrame,
        ReplayCurrentDurableFrame,
        LateOldDeregister,
    }

    const ACTIONS: [Action; 6] = [
        Action::RegisterReplacement,
        Action::LateOldDirectFrame,
        Action::LateOldDurableFrame,
        Action::CurrentDirectFrame,
        Action::ReplayCurrentDurableFrame,
        Action::LateOldDeregister,
    ];

    #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
    struct World {
        lineage: u8,
        current_generation: u8,
        live_heads: u8,
        boundary_markers: u8,
        document_copies: u8,
        operator_prompt_copies: u8,
        old_edit_applied: bool,
        current_edit_applied: bool,
        retired_frame_mutated_replacement: bool,
    }

    impl World {
        fn initial() -> Self {
            Self {
                lineage: 1,
                current_generation: 0,
                live_heads: 1,
                boundary_markers: 1,
                document_copies: 1,
                operator_prompt_copies: 1,
                old_edit_applied: false,
                current_edit_applied: false,
                retired_frame_mutated_replacement: false,
            }
        }

        fn step(&mut self, action: Action) {
            match action {
                Action::RegisterReplacement if self.current_generation < 2 => {
                    // Production serializes concurrent refresh registrations at the
                    // per-document generation fence, then performs retire + lineage
                    // rotation + register under the hub lock.
                    self.current_generation += 1;
                    self.lineage += 1;
                    self.live_heads = 1;
                }
                Action::LateOldDirectFrame | Action::LateOldDurableFrame
                    if self.current_generation == 0 =>
                {
                    self.old_edit_applied = true;
                }
                Action::LateOldDirectFrame | Action::LateOldDurableFrame => {
                    // Raw-identity generation fencing rejects the direct frame;
                    // lineage fencing quarantines the durable frame.
                }
                Action::CurrentDirectFrame | Action::ReplayCurrentDurableFrame
                    if self.current_generation >= 1 =>
                {
                    self.current_edit_applied = true;
                }
                Action::LateOldDeregister => {
                    // A deregister for the retired raw identity cannot remove
                    // the replacement logical replica.
                }
                _ => {}
            }
            self.assert_invariants();
        }

        fn assert_invariants(&self) {
            assert_eq!(
                self.live_heads, 1,
                "refresh created an independent head: {self:?}"
            );
            assert_eq!(
                self.document_copies, 1,
                "refresh replay duplicated the document: {self:?}",
            );
            assert_eq!(
                self.boundary_markers, 1,
                "refresh replay duplicated the exchange boundary: {self:?}",
            );
            assert_eq!(
                self.operator_prompt_copies, 1,
                "refresh lost or duplicated operator text: {self:?}",
            );
            assert!(
                !self.retired_frame_mutated_replacement,
                "retired generation mutated the replacement canonical: {self:?}",
            );
        }
    }

    fn explore(world: World, depth: usize, visited: &mut BTreeSet<World>) {
        world.assert_invariants();
        if depth == 0 || !visited.insert(world.clone()) {
            return;
        }
        for action in ACTIONS {
            let mut next = world.clone();
            next.step(action);
            explore(next, depth - 1, visited);
        }
    }

    #[test]
    fn exhaustive_refresh_interleavings_keep_one_head_and_one_document() {
        let mut visited = BTreeSet::new();
        explore(World::initial(), 10, &mut visited);
        assert!(visited.iter().any(|world| world.current_generation == 1));
        assert!(visited.iter().any(|world| world.current_generation == 2));
        assert!(visited.iter().any(|world| world.old_edit_applied));
        assert!(visited.iter().any(|world| world.current_edit_applied));
    }
}

/// Interaction model for the live failure observed in JetBrains: a response is
/// accepted while Compact Exchange/finalize waits on delivery, external CRDT
/// events arrive during ACK backoff, and the controller may recycle before the
/// retained target is committed. The world calls the production completion,
/// retry-admission, and capture-materialization policies.
mod retained_closeout_recovery_model {
    use agent_doc_document_realtime::write_policy::{
        CrdtRetryAdmission, CrdtWriteCompletion, CrdtWriteCompletionEvidence,
        decide_crdt_retry_admission, decide_crdt_write_completion,
    };
    use agent_doc_workflow::capture::{
        CaptureCloseoutMaterializationDecision, CaptureCloseoutMaterializationEvidence,
        decide_capture_closeout_materialization,
    };
    use std::collections::BTreeSet;

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
    enum ControllerSocket {
        #[default]
        Live,
        Stale,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Action {
        CaptureResponse,
        Finalize,
        CompactExchange,
        StartAsyncRecovery,
        EditorAck,
        ForegroundAckDeadline,
        RetryFinalize,
        LoseCanonical,
        BeginAckBackoff,
        ExternalAckEvent,
        EndAckBackoff,
        RecycleController,
        EnsureController,
        Commit,
    }

    const ACTIONS: [Action; 14] = [
        Action::CaptureResponse,
        Action::Finalize,
        Action::CompactExchange,
        Action::StartAsyncRecovery,
        Action::EditorAck,
        Action::ForegroundAckDeadline,
        Action::RetryFinalize,
        Action::LoseCanonical,
        Action::BeginAckBackoff,
        Action::ExternalAckEvent,
        Action::EndAckBackoff,
        Action::RecycleController,
        Action::EnsureController,
        Action::Commit,
    ];

    #[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
    struct World {
        active_capture: bool,
        exact_response_retained: bool,
        response_cells: u8,
        response_updates: u8,
        response_inline: bool,
        exact_archive: bool,
        editor_acked: bool,
        async_recovery_active: bool,
        foreground_success: bool,
        ack_backoff: bool,
        controller_ack_requests: u8,
        controller_socket: ControllerSocket,
        committed: bool,
    }

    #[derive(Debug, Default)]
    struct Coverage {
        retained_timeout_succeeded: bool,
        missing_retention_blocked: bool,
        retry_was_idempotent: bool,
        exact_replay_emitted_no_update: bool,
        backoff_gated_external_event: bool,
        stale_socket_blocked_commit: bool,
        exact_archive_committed: bool,
    }

    impl World {
        fn retain_response_cell(&mut self) {
            self.active_capture = true;
            self.exact_response_retained = true;
            self.response_cells = 1;
        }

        fn completion(&self) -> CrdtWriteCompletion {
            decide_crdt_write_completion(CrdtWriteCompletionEvidence {
                exact_target_retained: self.exact_response_retained
                    && (self.response_inline || self.exact_archive),
                async_delivery_recovery_active: self.async_recovery_active,
                delivery_converged: self.editor_acked,
            })
        }

        fn step(&mut self, action: Action, coverage: &mut Coverage) {
            if self.committed {
                return;
            }
            match action {
                Action::CaptureResponse => self.retain_response_cell(),
                Action::Finalize | Action::RetryFinalize => {
                    let cells_before = self.response_cells;
                    let updates_before = self.response_updates;
                    let already_exact_inline = self.active_capture
                        && self.exact_response_retained
                        && self.response_cells == 1
                        && self.response_inline;
                    self.retain_response_cell();
                    if !already_exact_inline {
                        self.response_inline = true;
                        self.exact_archive = false;
                        self.editor_acked = false;
                        self.response_updates = self.response_updates.saturating_add(1);
                    }
                    if action == Action::RetryFinalize && cells_before == 1 {
                        coverage.retry_was_idempotent = self.response_cells == 1;
                    }
                    if action == Action::RetryFinalize && already_exact_inline {
                        coverage.exact_replay_emitted_no_update =
                            self.response_updates == updates_before;
                    }
                }
                Action::CompactExchange => {
                    if self.active_capture && self.exact_response_retained {
                        self.response_inline = false;
                        self.exact_archive = true;
                        self.editor_acked = false;
                    }
                }
                Action::StartAsyncRecovery => self.async_recovery_active = true,
                Action::EditorAck => {
                    if self.exact_response_retained {
                        self.editor_acked = true;
                    }
                }
                Action::ForegroundAckDeadline => match self.completion() {
                    CrdtWriteCompletion::VisibleAndAcknowledged => {
                        self.foreground_success = true;
                    }
                    CrdtWriteCompletion::RetainedForAsyncDelivery => {
                        self.foreground_success = true;
                        coverage.retained_timeout_succeeded = true;
                    }
                    CrdtWriteCompletion::BlockMissingRetention => {
                        coverage.missing_retention_blocked = true;
                    }
                },
                Action::LoseCanonical => {
                    self.exact_response_retained = false;
                    self.editor_acked = false;
                    self.foreground_success = false;
                }
                Action::BeginAckBackoff => self.ack_backoff = true,
                Action::ExternalAckEvent => {
                    let before = self.controller_ack_requests;
                    if decide_crdt_retry_admission(self.ack_backoff)
                        == CrdtRetryAdmission::StartDrain
                    {
                        self.controller_ack_requests =
                            self.controller_ack_requests.saturating_add(1);
                    }
                    if self.ack_backoff {
                        coverage.backoff_gated_external_event =
                            self.controller_ack_requests == before;
                    }
                }
                Action::EndAckBackoff => self.ack_backoff = false,
                Action::RecycleController => {
                    self.controller_socket = ControllerSocket::Stale;
                }
                Action::EnsureController => {
                    self.controller_socket = ControllerSocket::Live;
                }
                Action::Commit => {
                    let materialization = decide_capture_closeout_materialization(
                        CaptureCloseoutMaterializationEvidence {
                            active_capture: self.active_capture,
                            capture_terminal: self.committed,
                            commit_surface_available: self.response_inline || self.exact_archive,
                            response_in_commit_surface: self.response_inline,
                            response_in_referenced_compact_archive: self.exact_archive,
                        },
                    );
                    if self.controller_socket == ControllerSocket::Stale {
                        coverage.stale_socket_blocked_commit = true;
                    } else if self.foreground_success
                        && self.exact_response_retained
                        && (self.response_inline || self.exact_archive)
                        && matches!(
                            materialization,
                            CaptureCloseoutMaterializationDecision::Allow(_)
                        )
                    {
                        self.committed = true;
                        if self.exact_archive {
                            coverage.exact_archive_committed = true;
                        }
                    }
                }
            }
        }
    }

    fn explore(max_depth: usize, coverage: &mut Coverage) {
        let mut frontier = BTreeSet::from([World::default()]);
        let mut seen = frontier.clone();
        for _ in 0..max_depth {
            let mut next_frontier = BTreeSet::new();
            for world in frontier {
                assert!(
                    world.response_cells <= 1,
                    "stacked response cells: {world:?}"
                );
                assert!(
                    !world.committed
                        || (world.exact_response_retained
                            && world.response_cells == 1
                            && (world.response_inline || world.exact_archive)),
                    "commit lost the exact response: {world:?}"
                );
                for action in ACTIONS {
                    let mut next = world.clone();
                    next.step(action, coverage);
                    if seen.insert(next.clone()) {
                        next_frontier.insert(next);
                    }
                }
            }
            frontier = next_frontier;
        }
    }

    #[test]
    fn delayed_ack_compact_finalize_recycle_schedule_is_single_copy_and_recoverable() {
        let mut coverage = Coverage::default();
        let mut world = World::default();
        for action in [
            Action::CaptureResponse,
            Action::Finalize,
            Action::BeginAckBackoff,
            Action::ExternalAckEvent,
            Action::CompactExchange,
            Action::StartAsyncRecovery,
            Action::ForegroundAckDeadline,
            Action::RetryFinalize,
            Action::RetryFinalize,
            Action::RecycleController,
            Action::Commit,
            Action::EnsureController,
            Action::Commit,
        ] {
            world.step(action, &mut coverage);
        }
        assert_eq!(world.response_cells, 1);
        assert!(world.committed);
        assert!(coverage.retained_timeout_succeeded);
        assert!(coverage.retry_was_idempotent);
        assert!(coverage.exact_replay_emitted_no_update);
        assert!(coverage.backoff_gated_external_event);
        assert!(coverage.stale_socket_blocked_commit);
    }

    #[test]
    fn exhaustive_retained_closeout_recovery_schedules_preserve_invariants() {
        let mut coverage = Coverage::default();
        explore(8, &mut coverage);
        assert!(coverage.retained_timeout_succeeded);
        assert!(coverage.missing_retention_blocked);
        assert!(coverage.retry_was_idempotent);
        assert!(coverage.exact_replay_emitted_no_update);
        assert!(coverage.backoff_gated_external_event);
        assert!(coverage.stale_socket_blocked_commit);
        assert!(coverage.exact_archive_committed);
    }
}

/// SimWorld regression for retained closeout churn. Document convergence emits
/// a state edge; passage of time alone cannot re-run finalize. A separate
/// effect-retry receipt covers transient transport/process failures.
mod reactive_retained_finalize_model {
    use std::collections::BTreeSet;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Action {
        Capture,
        Resume,
        OperatorEdit,
        NativeSave,
        TimerTick,
        FailNextEffect,
        EffectRetryDue,
    }

    const ACTIONS: [Action; 7] = [
        Action::Capture,
        Action::Resume,
        Action::OperatorEdit,
        Action::NativeSave,
        Action::TimerTick,
        Action::FailNextEffect,
        Action::EffectRetryDue,
    ];

    #[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
    struct World {
        captured: bool,
        authority_revision: u8,
        disk_revision: u8,
        state_epoch: u8,
        consumed_state_epoch: u8,
        effect_retry_epoch: u8,
        consumed_effect_retry_epoch: u8,
        fail_next_effect: bool,
        effect_retry_pending: bool,
        attempts: u8,
        committed: bool,
    }

    impl World {
        fn ready(&self) -> bool {
            self.captured
                && !self.committed
                && (self.state_epoch > self.consumed_state_epoch
                    || self.effect_retry_epoch > self.consumed_effect_retry_epoch)
        }

        fn step(&mut self, action: Action) {
            let attempts_before = self.attempts;
            match action {
                Action::Capture if !self.captured => {
                    self.captured = true;
                    self.authority_revision = 1;
                    self.state_epoch = self.state_epoch.saturating_add(1);
                }
                Action::Resume if self.ready() => {
                    self.attempts = self.attempts.saturating_add(1);
                    self.consumed_state_epoch = self.state_epoch;
                    self.consumed_effect_retry_epoch = self.effect_retry_epoch;
                    if self.fail_next_effect {
                        self.fail_next_effect = false;
                        self.effect_retry_pending = true;
                    } else if self.authority_revision == self.disk_revision {
                        self.committed = true;
                    }
                }
                Action::OperatorEdit if self.captured && !self.committed => {
                    self.authority_revision = self.authority_revision.saturating_add(1);
                }
                Action::NativeSave if self.captured && !self.committed => {
                    self.disk_revision = self.authority_revision;
                    // Production's retained-write Computed emits
                    // DocumentWriteConverged, which is the controller wake edge.
                    self.state_epoch = self.state_epoch.saturating_add(1);
                }
                Action::FailNextEffect if self.captured && !self.committed => {
                    self.fail_next_effect = true;
                }
                Action::EffectRetryDue if self.effect_retry_pending => {
                    self.effect_retry_pending = false;
                    self.effect_retry_epoch = self.effect_retry_epoch.saturating_add(1);
                }
                Action::TimerTick
                | Action::Capture
                | Action::Resume
                | Action::OperatorEdit
                | Action::NativeSave
                | Action::FailNextEffect
                | Action::EffectRetryDue => {}
            }
            if action == Action::TimerTick {
                assert_eq!(
                    self.attempts, attempts_before,
                    "time passage retried finalize without a Source edge: {self:?}"
                );
            }
        }
    }

    #[test]
    fn native_save_state_edge_rearms_once_without_timer_retry() {
        let mut world = World::default();
        for action in [
            Action::Capture,
            Action::Resume,
            Action::TimerTick,
            Action::TimerTick,
            Action::OperatorEdit,
            Action::TimerTick,
        ] {
            world.step(action);
        }
        assert_eq!(world.attempts, 1);
        assert!(!world.committed);

        world.step(Action::NativeSave);
        world.step(Action::Resume);
        assert_eq!(world.attempts, 2);
        assert!(world.committed);
    }

    #[test]
    fn exhaustive_schedules_never_turn_timer_ticks_into_finalize_attempts() {
        let mut frontier = BTreeSet::from([World::default()]);
        let mut seen = frontier.clone();
        for _ in 0..9 {
            let mut next_frontier = BTreeSet::new();
            for world in frontier {
                for action in ACTIONS {
                    let mut next = world.clone();
                    next.step(action);
                    if seen.insert(next.clone()) {
                        next_frontier.insert(next);
                    }
                }
            }
            frontier = next_frontier;
        }
        assert!(seen.iter().any(|world| world.committed));
        assert!(seen.iter().any(|world| world.effect_retry_epoch > 0));
    }
}

/// Deterministic model for the queue-strike loss seen when the live editor and
/// disk differed only in route-owned queue state. Delivery ACK, native editor
/// save, and Git commit are distinct transitions.
mod editor_commit_projection_model {
    use agent_doc_controller_io::project_controller::{
        ControllerCommitProjectionDecision, decide_controller_commit_projection,
    };
    use agent_doc_crdt_relay_io::CurrentText;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Action {
        StrikeQueue,
        DeliveryAck,
        Commit,
        OperatorEdit,
        NativeSave,
    }

    #[derive(Clone, Debug)]
    struct World {
        authority: String,
        disk: String,
        delivery_converged: bool,
        native_save_requested: bool,
        committed: bool,
    }

    impl Default for World {
        fn default() -> Self {
            let initial = "# Queue\n- [ ] [#implementlazilyintent] implement\n";
            Self {
                authority: initial.into(),
                disk: initial.into(),
                delivery_converged: false,
                native_save_requested: false,
                committed: false,
            }
        }
    }

    impl World {
        fn decision(&self) -> ControllerCommitProjectionDecision {
            decide_controller_commit_projection(
                self.delivery_converged,
                &CurrentText::Current {
                    text: self.authority.clone(),
                    live_editors: 1,
                    delivery_converged: self.delivery_converged,
                    delivery_version: u64::from(self.delivery_converged),
                    semantics: None,
                },
                Some(self.disk.as_bytes()),
            )
        }

        fn step(&mut self, action: Action) {
            if self.committed {
                return;
            }
            match action {
                Action::StrikeQueue => {
                    self.authority = "# Queue\n- [x] [#implementlazilyintent] implement\n".into();
                    self.delivery_converged = false;
                }
                Action::DeliveryAck => self.delivery_converged = true,
                Action::Commit => match self.decision() {
                    ControllerCommitProjectionDecision::Ready => self.committed = true,
                    ControllerCommitProjectionDecision::NativeSaveRequired => {
                        self.native_save_requested = true;
                    }
                    ControllerCommitProjectionDecision::AwaitConvergence => {}
                },
                Action::OperatorEdit => {
                    self.authority
                        .push_str("\noperator text typed during closeout\n");
                    self.delivery_converged = true;
                }
                Action::NativeSave if self.native_save_requested => {
                    // The editor saves its latest authoritative buffer, including
                    // operator text that arrived after the original request.
                    self.disk = self.authority.clone();
                    self.native_save_requested = false;
                }
                Action::NativeSave => {}
            }
            assert!(
                !self.committed
                    || (self.delivery_converged
                        && self.disk.as_bytes() == self.authority.as_bytes()),
                "Git committed before the exact live editor projection reached disk: {self:?}"
            );
        }
    }

    #[test]
    fn queue_strike_waits_for_native_save_and_preserves_concurrent_operator_text() {
        let mut world = World::default();
        world.step(Action::StrikeQueue);
        world.step(Action::DeliveryAck);
        world.step(Action::Commit);
        assert!(!world.committed);
        assert!(world.native_save_requested);
        assert_eq!(
            world.decision(),
            ControllerCommitProjectionDecision::NativeSaveRequired
        );

        world.step(Action::OperatorEdit);
        world.step(Action::NativeSave);
        world.step(Action::Commit);

        assert!(world.committed);
        assert!(world.disk.contains("[x] [#implementlazilyintent]"));
        assert!(world.disk.contains("operator text typed during closeout"));
    }
}

/// SimWorld reproduction for a restarted-editor recovery wedge: a retained
/// response has reached `write_applied`, the editor owns a newer authoritative
/// buffer, disk remains one cut behind, and an idle controller does not
/// manufacture another write. The actual editor-delivery ACK must trigger one
/// native save; neither a single connected editor nor timer ticks are proof.
mod read_only_retained_projection_model {
    use agent_doc_session_check_io::command::{
        ReadOnlyRetainedCloseoutResumeProjection, ReadOnlyTerminalProjectionDecision,
        decide_read_only_terminal_projection,
    };
    use agent_doc_turn::CyclePhase;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Action {
        SessionCheck,
        TimerTick,
        EditorDeliveryAck,
        StaleNativeSaveReceipt,
        ExactNativeSaveReceipt,
    }

    struct World {
        authority: String,
        disk: String,
        phase: CyclePhase,
        retained_write_blocks: bool,
        delivery_converged: bool,
        native_save_requested: bool,
        native_save_requests: usize,
        committed: bool,
        response_replays: usize,
        operator_exit_requested: bool,
        retained_closeout_resume: ReadOnlyRetainedCloseoutResumeProjection,
    }

    impl World {
        fn retained_write_applied_wedge() -> Self {
            let authority = "orchard offer response\nrestored operator note\n".to_string();
            let disk = "retained response\n".to_string();
            let phase = CyclePhase::WriteApplied;
            let retained_write_blocks = true;
            let retained_closeout_resume = ReadOnlyRetainedCloseoutResumeProjection::new(
                authority == disk,
                Some(phase),
                retained_write_blocks,
            );
            Self {
                authority,
                disk,
                phase,
                retained_write_blocks,
                delivery_converged: false,
                native_save_requested: false,
                native_save_requests: 0,
                committed: false,
                response_replays: 0,
                operator_exit_requested: false,
                retained_closeout_resume,
            }
        }

        fn evaluate_projection(&mut self) {
            match decide_read_only_terminal_projection(
                self.authority == self.disk,
                Some(self.phase),
                self.retained_write_blocks,
                self.delivery_converged,
            ) {
                ReadOnlyTerminalProjectionDecision::Converged
                | ReadOnlyTerminalProjectionDecision::AwaitEditorDelivery => {}
                ReadOnlyTerminalProjectionDecision::RequestNativeEditorSave => {
                    if !self.native_save_requested {
                        self.native_save_requested = true;
                        self.native_save_requests += 1;
                    }
                }
                ReadOnlyTerminalProjectionDecision::ObserveOnly => {
                    self.operator_exit_requested = true;
                }
            }
        }

        fn step(&mut self, action: Action) {
            match action {
                Action::SessionCheck => self.evaluate_projection(),
                // A clock is not a document-state Source. It cannot create
                // another closeout or native-save Effect.
                Action::TimerTick => {}
                Action::EditorDeliveryAck => {
                    self.delivery_converged = true;
                    self.evaluate_projection();
                }
                Action::StaleNativeSaveReceipt if self.native_save_requested => {
                    self.retained_closeout_resume
                        .observe_native_save(false, false);
                }
                Action::ExactNativeSaveReceipt if self.native_save_requested => {
                    self.disk = self.authority.clone();
                    self.native_save_requested = false;
                    self.retained_closeout_resume
                        .observe_native_save(true, self.authority == self.disk);
                    if self.retained_closeout_resume.should_resume() {
                        self.phase = CyclePhase::Committed;
                        self.retained_write_blocks = false;
                        self.committed = true;
                    }
                }
                _ => {}
            }
            assert!(
                !self.committed || self.authority == self.disk,
                "retained closeout committed before exact editor-owned save",
            );
        }
    }

    #[test]
    fn orchard_offer_waits_for_delivery_edge_and_saves_exactly_once() {
        let mut world = World::retained_write_applied_wedge();
        world.step(Action::SessionCheck);
        assert!(!world.native_save_requested);
        assert_eq!(world.native_save_requests, 0);
        assert!(!world.operator_exit_requested);

        for _ in 0..20 {
            world.step(Action::TimerTick);
        }
        assert_eq!(world.native_save_requests, 0);

        world.step(Action::EditorDeliveryAck);
        assert!(world.native_save_requested);
        assert_eq!(world.native_save_requests, 1);
        world.step(Action::SessionCheck);
        assert_eq!(world.native_save_requests, 1);

        world.step(Action::StaleNativeSaveReceipt);
        assert!(!world.committed);
        assert_eq!(world.response_replays, 0);

        world.step(Action::ExactNativeSaveReceipt);
        assert!(world.disk.contains("restored operator note"));

        assert!(world.committed);
        assert_eq!(world.phase, CyclePhase::Committed);
        assert_eq!(world.response_replays, 0);
        assert!(!world.operator_exit_requested);
    }
}

/// SimWorld for the mid-turn editor-plugin update failure class. It couples the
/// production generation-refresh and authoritative-rebase policies with durable
/// response/steering state. A native-only reload is intentionally a separate
/// action: it cannot advance the editor-plugin generation.
mod plugin_generation_capture_rebase_model {
    use agent_doc_workflow::capture::{
        AuthoritativeReplayRebaseDecision, AuthoritativeReplayRebaseEvidence,
        PluginGenerationRefreshDecision, PluginGenerationRefreshEvidence,
        decide_authoritative_replay_rebase, decide_plugin_generation_refresh,
    };
    use std::collections::BTreeSet;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Action {
        CaptureResponse,
        AddSteering,
        ConflictingEdit,
        MakeInstall,
        RestartEditor,
        ReloadNativeOnly,
        RevalidateLiveGeneration,
        RebaseCapture,
        ReplayResponse,
        Commit,
    }

    const ACTIONS: [Action; 10] = [
        Action::CaptureResponse,
        Action::AddSteering,
        Action::ConflictingEdit,
        Action::MakeInstall,
        Action::RestartEditor,
        Action::ReloadNativeOnly,
        Action::RevalidateLiveGeneration,
        Action::RebaseCapture,
        Action::ReplayResponse,
        Action::Commit,
    ];

    #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
    struct World {
        source_plugin_generation: u8,
        installed_plugin_generations: [u8; 2],
        install_completed: bool,
        preflight_plugin_generation: u8,
        live_plugin_generation: u8,
        recognized_plugin_generation: u8,
        native_generation: u8,
        active_capture: bool,
        response_retained: bool,
        response_applied: bool,
        response_apply_count: u8,
        steering_durable: bool,
        baseline_drifted: bool,
        monotonic_extension: bool,
        committed: bool,
    }

    impl Default for World {
        fn default() -> Self {
            Self {
                source_plugin_generation: 2,
                installed_plugin_generations: [1, 1],
                install_completed: false,
                preflight_plugin_generation: 1,
                live_plugin_generation: 1,
                recognized_plugin_generation: 1,
                native_generation: 1,
                active_capture: false,
                response_retained: false,
                response_applied: false,
                response_apply_count: 0,
                steering_durable: false,
                baseline_drifted: false,
                monotonic_extension: true,
                committed: false,
            }
        }
    }

    #[derive(Debug, Default)]
    struct Coverage {
        make_install_converged_existing_packages: bool,
        make_install_did_not_claim_live_activation: bool,
        live_generation_superseded_preflight: bool,
        native_reload_did_not_upgrade_plugin: bool,
        steering_rebase_replayed_once: bool,
        conflict_blocked_rebase: bool,
    }

    impl World {
        fn step(&mut self, action: Action, coverage: &mut Coverage) {
            if self.committed {
                return;
            }
            match action {
                Action::CaptureResponse => {
                    self.active_capture = true;
                    self.response_retained = true;
                }
                Action::AddSteering => {
                    self.steering_durable = true;
                    self.baseline_drifted = true;
                    self.monotonic_extension = true;
                }
                Action::ConflictingEdit => {
                    self.baseline_drifted = true;
                    self.monotonic_extension = false;
                }
                Action::MakeInstall => {
                    let live_before = self.live_plugin_generation;
                    self.installed_plugin_generations = [self.source_plugin_generation; 2];
                    self.native_generation = self.source_plugin_generation;
                    self.install_completed = true;
                    coverage.make_install_converged_existing_packages = self
                        .installed_plugin_generations
                        .iter()
                        .all(|generation| *generation == self.source_plugin_generation);
                    coverage.make_install_did_not_claim_live_activation =
                        self.live_plugin_generation == live_before;
                }
                Action::RestartEditor => {
                    if self.install_completed {
                        self.live_plugin_generation = self.installed_plugin_generations[0];
                    }
                }
                Action::ReloadNativeOnly => {
                    let before = self.live_plugin_generation;
                    self.native_generation = 2;
                    coverage.native_reload_did_not_upgrade_plugin =
                        self.live_plugin_generation == before;
                }
                Action::RevalidateLiveGeneration => {
                    let decision =
                        decide_plugin_generation_refresh(PluginGenerationRefreshEvidence {
                            preflight_generation: self.preflight_plugin_generation.into(),
                            live_generation: self.live_plugin_generation.into(),
                            live_registration_observed: true,
                        });
                    if decision == PluginGenerationRefreshDecision::AdoptLive {
                        self.recognized_plugin_generation = self.live_plugin_generation;
                        coverage.live_generation_superseded_preflight = true;
                    }
                }
                Action::RebaseCapture => {
                    let decision =
                        decide_authoritative_replay_rebase(AuthoritativeReplayRebaseEvidence {
                            capture_repairable: self.active_capture,
                            replay_baseline_drifted: self.baseline_drifted,
                            authoritative_current: true,
                            matching_open_cycle: self.active_capture,
                            captured_response_body_missing: !self.response_applied,
                            captured_response_heading_answered: false,
                            current_monotonically_extends_baseline: self.monotonic_extension,
                        });
                    match decision {
                        AuthoritativeReplayRebaseDecision::RebaseToAuthoritativeCurrent => {
                            self.baseline_drifted = false;
                        }
                        AuthoritativeReplayRebaseDecision::BlockConflict => {
                            if !self.monotonic_extension {
                                coverage.conflict_blocked_rebase = true;
                            }
                        }
                        AuthoritativeReplayRebaseDecision::KeepBaseline => {}
                    }
                }
                Action::ReplayResponse => {
                    if self.active_capture
                        && self.response_retained
                        && !self.baseline_drifted
                        && self.recognized_plugin_generation == self.live_plugin_generation
                    {
                        self.response_applied = true;
                        self.response_apply_count = 1;
                    }
                }
                Action::Commit => {
                    if self.response_applied
                        && self.response_apply_count == 1
                        && (!self.steering_durable || !self.baseline_drifted)
                    {
                        self.committed = true;
                        if self.steering_durable {
                            coverage.steering_rebase_replayed_once = true;
                        }
                    }
                }
            }
        }
    }

    fn assert_invariants(world: &World) {
        assert!(
            !world.install_completed
                || world
                    .installed_plugin_generations
                    .iter()
                    .all(|generation| *generation == world.source_plugin_generation),
            "make install left an existing package stale: {world:?}"
        );
        assert!(
            world.response_apply_count <= 1,
            "duplicate response: {world:?}"
        );
        assert!(
            !world.active_capture || world.response_retained,
            "active capture lost its response: {world:?}"
        );
        assert!(
            world.recognized_plugin_generation <= world.live_plugin_generation,
            "recognized a plugin generation that never registered: {world:?}"
        );
        assert!(
            !world.committed || (world.response_applied && world.response_apply_count == 1),
            "commit lost or duplicated the response: {world:?}"
        );
    }

    fn explore(max_depth: usize, coverage: &mut Coverage) {
        let mut frontier = BTreeSet::from([World::default()]);
        let mut seen = frontier.clone();
        for _ in 0..max_depth {
            let mut next_frontier = BTreeSet::new();
            for world in frontier {
                assert_invariants(&world);
                for action in ACTIONS {
                    let mut next = world.clone();
                    next.step(action, coverage);
                    assert_invariants(&next);
                    if seen.insert(next.clone()) {
                        next_frontier.insert(next);
                    }
                }
            }
            frontier = next_frontier;
        }
    }

    #[test]
    fn mid_turn_plugin_update_with_later_steering_replays_and_commits_once() {
        let mut world = World::default();
        let mut coverage = Coverage::default();
        for action in [
            Action::CaptureResponse,
            Action::AddSteering,
            Action::ReloadNativeOnly,
            Action::MakeInstall,
            Action::RestartEditor,
            Action::RevalidateLiveGeneration,
            Action::RebaseCapture,
            Action::ReplayResponse,
            Action::Commit,
        ] {
            world.step(action, &mut coverage);
            assert_invariants(&world);
        }
        assert!(world.committed);
        assert!(world.steering_durable);
        assert!(coverage.make_install_converged_existing_packages);
        assert!(coverage.make_install_did_not_claim_live_activation);
        assert!(coverage.live_generation_superseded_preflight);
        assert!(coverage.native_reload_did_not_upgrade_plugin);
        assert!(coverage.steering_rebase_replayed_once);
    }

    #[test]
    fn exhaustive_plugin_update_rebase_schedules_preserve_invariants() {
        let mut coverage = Coverage::default();
        explore(11, &mut coverage);
        assert!(coverage.make_install_converged_existing_packages);
        assert!(coverage.make_install_did_not_claim_live_activation);
        assert!(coverage.live_generation_superseded_preflight);
        assert!(coverage.native_reload_did_not_upgrade_plugin);
        assert!(coverage.steering_rebase_replayed_once);
        assert!(coverage.conflict_blocked_rebase);
    }
}

/// Cross-layer reference kernel for realtime delivery, editor IPC, and cycle
/// closeout. This deliberately models observable policy rather than sockets,
/// IDE APIs, or a second production implementation. Generated schedules make
/// retained delivery identity, controller epochs, semantic response cells, and
/// the durable commit barrier share one state space.
mod realtime_ipc_cycle_model {
    use std::collections::{BTreeMap, BTreeSet, VecDeque};

    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    enum Structure {
        Exact,
        Invalid,
        RepairRequired,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Fault {
        None,
        DropOnce,
        DuplicateOnce,
        DelayOnce,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Phase {
        Idle,
        Captured,
        Projected,
        Interrupted,
        Committed,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum AmbiguityEvidence {
        SameSemanticOperation,
        EditorCausallyNewer,
        ReplicaCausallyNewer,
        ConcurrentCrdtCompatible,
        ConcurrentSemanticConflict,
        MissingCausalProof,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum AmbiguityResolution {
        Dedupe,
        ChooseEditor,
        ChooseReplica,
        MergeCrdt,
        NeedsOperator,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum QueueMaintenanceObservation {
        LiveHead,
        StaleFirstAdditionOnly,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct QueueRecoveryArtifact {
        reason: &'static str,
        preserved_live_items: BTreeSet<u8>,
    }

    fn resolve_ambiguity(evidence: AmbiguityEvidence) -> AmbiguityResolution {
        match evidence {
            AmbiguityEvidence::SameSemanticOperation => AmbiguityResolution::Dedupe,
            AmbiguityEvidence::EditorCausallyNewer => AmbiguityResolution::ChooseEditor,
            AmbiguityEvidence::ReplicaCausallyNewer => AmbiguityResolution::ChooseReplica,
            AmbiguityEvidence::ConcurrentCrdtCompatible => AmbiguityResolution::MergeCrdt,
            AmbiguityEvidence::ConcurrentSemanticConflict
            | AmbiguityEvidence::MissingCausalProof => AmbiguityResolution::NeedsOperator,
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Frame {
        response_id: u8,
        generation: u64,
        expected_editor: u32,
        target: u32,
        structure: Structure,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Delivery {
        frame: Frame,
        visible: bool,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Action {
        Capture {
            response_id: u8,
            already_visible: bool,
            structure: Structure,
        },
        SetFault(Fault),
        SendRetained,
        ReleaseDelayed,
        DeliverOne,
        Project,
        Ack,
        UserEdit,
        AddQueueItem(u8),
        RunQueueMaintenance(QueueMaintenanceObservation),
        SaveEditor,
        ExternalDiskChange,
        AcceptPendingDisk,
        ControllerRecycle,
        RegisterReplica,
        Interrupt,
        CloseEditor,
        OpenEditor,
        ForceDisk,
        ResolveAmbiguity(AmbiguityEvidence),
        Commit,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    enum Capability {
        RetainedDelivery,
        KeyedSingleFlight,
        AuthorityEpochFence,
        SemanticIdempotence,
        BoundedRetry,
        CancellationPropagation,
        DurableEffectBarrier,
        PendingExternalDiskDecision,
        EvidenceBasedAmbiguityResolution,
    }

    impl Action {
        fn capability(self) -> Capability {
            match self {
                Self::Capture { .. } => Capability::SemanticIdempotence,
                Self::SetFault(_) | Self::SendRetained | Self::ReleaseDelayed => {
                    Capability::RetainedDelivery
                }
                Self::DeliverOne => Capability::KeyedSingleFlight,
                Self::Project | Self::ControllerRecycle | Self::RegisterReplica => {
                    Capability::AuthorityEpochFence
                }
                Self::Ack
                | Self::UserEdit
                | Self::AddQueueItem(_)
                | Self::OpenEditor
                | Self::CloseEditor => Capability::BoundedRetry,
                Self::RunQueueMaintenance(_) => Capability::AuthorityEpochFence,
                Self::Interrupt => Capability::CancellationPropagation,
                Self::SaveEditor | Self::ForceDisk | Self::Commit => {
                    Capability::DurableEffectBarrier
                }
                Self::ExternalDiskChange | Self::AcceptPendingDisk => {
                    Capability::PendingExternalDiskDecision
                }
                Self::ResolveAmbiguity(_) => Capability::EvidenceBasedAmbiguityResolution,
            }
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct Observation {
        canonical: u32,
        editor: u32,
        disk: u32,
        pending_disk: Option<u32>,
        head: u32,
        controller_epoch: u64,
        replica_epoch: Option<u64>,
        acked_generation: u64,
        response_cells: Vec<u8>,
        phase: Phase,
    }

    #[derive(Debug)]
    struct World {
        canonical: u32,
        editor: u32,
        disk: u32,
        pending_disk: Option<u32>,
        head: u32,
        controller_epoch: u64,
        replica_epoch: Option<u64>,
        live_editor: bool,
        editor_dirty: bool,
        next_generation: u64,
        retained: Option<Frame>,
        delayed: VecDeque<Frame>,
        ipc: VecDeque<Frame>,
        delivery: Option<Delivery>,
        decoded: BTreeMap<(u8, u64), usize>,
        visible_frontiers: BTreeSet<(u8, u64)>,
        acked_generation: u64,
        ack_proof_valid: bool,
        response_cells: Vec<u8>,
        canonical_queue_items: BTreeMap<u8, usize>,
        editor_queue_items: BTreeMap<u8, usize>,
        disk_queue_items: BTreeMap<u8, usize>,
        head_queue_items: BTreeMap<u8, usize>,
        accepted_queue_items: BTreeSet<u8>,
        queue_recovery_artifacts: Vec<QueueRecoveryArtifact>,
        pending_responses: BTreeSet<u8>,
        phase: Phase,
        fault: Fault,
        retry_scheduled: bool,
        unsafe_force_disk_writes: usize,
        trace: Vec<Action>,
    }

    impl World {
        fn new() -> Self {
            Self {
                canonical: 1,
                editor: 1,
                disk: 1,
                pending_disk: None,
                head: 1,
                controller_epoch: 1,
                replica_epoch: Some(1),
                live_editor: true,
                editor_dirty: false,
                next_generation: 0,
                retained: None,
                delayed: VecDeque::new(),
                ipc: VecDeque::new(),
                delivery: None,
                decoded: BTreeMap::new(),
                visible_frontiers: BTreeSet::new(),
                acked_generation: 0,
                ack_proof_valid: true,
                response_cells: Vec::new(),
                canonical_queue_items: BTreeMap::new(),
                editor_queue_items: BTreeMap::new(),
                disk_queue_items: BTreeMap::new(),
                head_queue_items: BTreeMap::new(),
                accepted_queue_items: BTreeSet::new(),
                queue_recovery_artifacts: Vec::new(),
                pending_responses: BTreeSet::new(),
                phase: Phase::Idle,
                fault: Fault::None,
                retry_scheduled: false,
                unsafe_force_disk_writes: 0,
                trace: Vec::new(),
            }
        }

        fn observe(&self) -> Observation {
            let mut response_cells = self.response_cells.clone();
            response_cells.sort_unstable();
            Observation {
                canonical: self.canonical,
                editor: self.editor,
                disk: self.disk,
                pending_disk: self.pending_disk,
                head: self.head,
                controller_epoch: self.controller_epoch,
                replica_epoch: self.replica_epoch,
                acked_generation: self.acked_generation,
                response_cells,
                phase: self.phase,
            }
        }

        fn make_frame(
            &mut self,
            response_id: u8,
            structure: Structure,
            already_visible: bool,
        ) -> Frame {
            self.next_generation += 1;
            let expected_editor = self.editor;
            if already_visible {
                self.insert_response_cell(response_id);
                self.editor = self.editor.saturating_add(1);
                self.canonical = self.editor;
            } else {
                self.canonical = self.canonical.max(self.editor).saturating_add(1);
            }
            Frame {
                response_id,
                generation: self.next_generation,
                expected_editor: if already_visible {
                    self.editor
                } else {
                    expected_editor
                },
                target: self.canonical,
                structure,
            }
        }

        fn insert_response_cell(&mut self, response_id: u8) {
            if !self.response_cells.contains(&response_id) {
                self.response_cells.push(response_id);
            }
        }

        fn apply(&mut self, action: Action) {
            self.trace.push(action);
            match action {
                Action::Capture {
                    response_id,
                    already_visible,
                    structure,
                } => {
                    self.pending_responses.insert(response_id);
                    let frame = self.make_frame(response_id, structure, already_visible);
                    self.retained = Some(frame);
                    self.phase = Phase::Captured;
                }
                Action::SetFault(fault) => self.fault = fault,
                Action::SendRetained => self.send_retained(),
                Action::ReleaseDelayed => {
                    while let Some(frame) = self.delayed.pop_front() {
                        self.ipc.push_back(frame);
                    }
                }
                Action::DeliverOne => self.deliver_one(),
                Action::Project => self.project(),
                Action::Ack => self.ack(),
                Action::UserEdit => {
                    self.editor = self.editor.saturating_add(1);
                    self.pending_disk = None;
                    self.invalidate_visible_delivery_for_operator_edit();
                    if self.live_editor && self.replica_epoch == Some(self.controller_epoch) {
                        self.canonical = self.editor;
                    }
                    if self.phase == Phase::Committed {
                        self.phase = Phase::Idle;
                    }
                    self.editor_dirty = true;
                    self.retry_scheduled = true;
                }
                Action::AddQueueItem(item_id) => {
                    if self.live_editor {
                        self.accepted_queue_items.insert(item_id);
                        self.editor_queue_items.insert(item_id, 1);
                        self.editor = self.editor.saturating_add(1);
                        self.pending_disk = None;
                        self.invalidate_visible_delivery_for_operator_edit();
                        if self.replica_epoch == Some(self.controller_epoch) {
                            self.canonical_queue_items.insert(item_id, 1);
                            self.canonical = self.editor;
                        }
                        if self.phase == Phase::Committed {
                            self.phase = Phase::Idle;
                        }
                        self.editor_dirty = true;
                        self.retry_scheduled = true;
                    }
                }
                Action::RunQueueMaintenance(observation) => match observation {
                    QueueMaintenanceObservation::LiveHead => {
                        if self.live_editor && self.replica_epoch == Some(self.controller_epoch) {
                            self.canonical_queue_items = self.editor_queue_items.clone();
                            self.canonical = self.editor;
                        }
                    }
                    QueueMaintenanceObservation::StaleFirstAdditionOnly => {
                        // The maintenance candidate was derived after the first
                        // operator addition but before later pre-run additions.
                        // Whole-head CAS rejects it; the live editor remains the
                        // recovery source and the durable artifact explains retry.
                        self.queue_recovery_artifacts.push(QueueRecoveryArtifact {
                            reason: "head_advanced_retry_from_live_head",
                            preserved_live_items: self.editor_queue_items.keys().copied().collect(),
                        });
                        self.retry_scheduled = true;
                    }
                },
                Action::SaveEditor => {
                    // Save advances durability only. It must never replay the
                    // already-admitted editor mutation into canonical state.
                    if self.live_editor {
                        self.disk = self.editor;
                        self.disk_queue_items = self.editor_queue_items.clone();
                        // Detached file-watch reflection may advance canonical,
                        // but semantic identity makes this the same mutation.
                        self.canonical = self.editor;
                        self.canonical_queue_items = self.editor_queue_items.clone();
                        self.editor_dirty = false;
                        // A save-flush is exact authority evidence: the bytes that
                        // reached disk came from the live editor, so any older
                        // external-disk candidate is superseded.
                        self.pending_disk = None;
                    }
                }
                Action::ExternalDiskChange => {
                    self.disk = self.disk.saturating_add(1);
                    if self.phase == Phase::Committed {
                        self.phase = Phase::Idle;
                    }
                    if self.live_editor {
                        // Keep the live buffer/canonical replica authoritative
                        // while the IDE presents its cache-conflict decision.
                        self.pending_disk = Some(self.disk);
                    } else {
                        self.canonical = self.disk;
                        self.canonical_queue_items = self.disk_queue_items.clone();
                        self.pending_disk = None;
                    }
                }
                Action::AcceptPendingDisk => {
                    if self.live_editor
                        && let Some(candidate) = self.pending_disk.take()
                    {
                        self.editor = candidate;
                        self.canonical = candidate;
                        self.editor_queue_items = self.disk_queue_items.clone();
                        self.canonical_queue_items = self.disk_queue_items.clone();
                        // The operator explicitly chose the disk-backed buffer in
                        // the IDE conflict UI. Queue items absent from that chosen
                        // buffer are deliberate replacements, not silent losses.
                        self.accepted_queue_items
                            .retain(|id| self.editor_queue_items.contains_key(id));
                        self.editor_dirty = false;
                    }
                }
                Action::ControllerRecycle => {
                    self.controller_epoch += 1;
                    self.replica_epoch = None;
                    self.delivery = None;
                    self.ipc.clear();
                    self.delayed.clear();
                    if let Some(mut retained) = self.retained {
                        self.next_generation += 1;
                        retained.generation = self.next_generation;
                        retained.expected_editor = self.editor;
                        retained.target = self.canonical;
                        self.retained = Some(retained);
                    }
                    self.retry_scheduled = true;
                }
                Action::RegisterReplica => {
                    if self.live_editor {
                        self.replica_epoch = Some(self.controller_epoch);
                        self.canonical = self.editor;
                        self.canonical_queue_items = self.editor_queue_items.clone();
                        self.retry_scheduled = false;
                    }
                }
                Action::Interrupt => {
                    if self.phase != Phase::Committed {
                        self.phase = Phase::Interrupted;
                    }
                }
                Action::CloseEditor => {
                    self.live_editor = false;
                    self.replica_epoch = None;
                    // Once the final editor closes there is no live-buffer
                    // authority to protect. Current disk becomes the fallback.
                    self.pending_disk = None;
                    self.canonical = self.disk;
                    self.canonical_queue_items = self.disk_queue_items.clone();
                }
                Action::OpenEditor => {
                    self.live_editor = true;
                    if self.phase == Phase::Committed {
                        self.editor = self.canonical;
                        self.editor_queue_items = self.canonical_queue_items.clone();
                    }
                    self.retry_scheduled = true;
                }
                Action::ForceDisk => {
                    if self.live_editor {
                        // An explicitly-authorized recovery write is still only a
                        // disk candidate while an editor is open. It cannot
                        // overwrite or component-merge the live buffer.
                        self.disk = self.canonical;
                        self.disk_queue_items = self.canonical_queue_items.clone();
                        self.pending_disk = (self.disk != self.editor).then_some(self.disk);
                    } else if self.pending_responses.is_empty() && !self.editor_dirty {
                        self.disk = self.canonical;
                        self.head = self.canonical;
                        self.disk_queue_items = self.canonical_queue_items.clone();
                        self.head_queue_items = self.canonical_queue_items.clone();
                        self.phase = Phase::Committed;
                    }
                }
                Action::ResolveAmbiguity(evidence) => {
                    // Rules may select/dedupe a causally proven authority or merge
                    // a compatible CRDT history. Missing lineage or competing
                    // semantic replacement intents must be a mutation-free stop.
                    let before = self.observe();
                    if resolve_ambiguity(evidence) == AmbiguityResolution::NeedsOperator {
                        assert_eq!(self.observe(), before);
                    }
                }
                Action::Commit => self.commit(),
            }
            self.assert_invariants();
        }

        fn invalidate_visible_delivery_for_operator_edit(&mut self) {
            let Some(mut delivery) = self.delivery else {
                return;
            };
            if !delivery.visible {
                return;
            }
            self.visible_frontiers
                .remove(&(delivery.frame.response_id, delivery.frame.generation));
            delivery.visible = false;
            self.delivery = Some(delivery);
        }

        fn send_retained(&mut self) {
            let Some(frame) = self.retained else {
                return;
            };
            match std::mem::replace(&mut self.fault, Fault::None) {
                Fault::None => self.ipc.push_back(frame),
                Fault::DropOnce => {}
                Fault::DuplicateOnce => {
                    self.ipc.push_back(frame);
                    self.ipc.push_back(frame);
                }
                Fault::DelayOnce => self.delayed.push_back(frame),
            }
        }

        fn deliver_one(&mut self) {
            if !self.live_editor || self.replica_epoch != Some(self.controller_epoch) {
                self.retry_scheduled = true;
                return;
            }
            let Some(frame) = self.ipc.pop_front() else {
                return;
            };
            if !self.pending_responses.contains(&frame.response_id) {
                // A late duplicate from an already committed semantic cell is
                // retired without reopening the cycle.
                if self.retained == Some(frame) {
                    self.retained = None;
                }
                return;
            }
            let key = (frame.response_id, frame.generation);
            if self.decoded.contains_key(&key) {
                return;
            }
            self.decoded.insert(key, 1);
            self.delivery = Some(Delivery {
                frame,
                visible: false,
            });
        }

        fn project(&mut self) {
            let Some(mut delivery) = self.delivery else {
                return;
            };
            if !self.live_editor || self.replica_epoch != Some(self.controller_epoch) {
                self.retry_scheduled = true;
                return;
            }
            match delivery.frame.structure {
                Structure::Exact => {
                    if self.editor != delivery.frame.expected_editor
                        && self.editor != delivery.frame.target
                    {
                        // The editor advanced. Retire the stale cut and
                        // recompose the same semantic response exactly once.
                        let response_id = delivery.frame.response_id;
                        let already_visible = self.response_cells.contains(&response_id);
                        let frame = self.make_frame(response_id, Structure::Exact, already_visible);
                        self.retained = Some(frame);
                        self.delivery = None;
                        self.ipc.clear();
                        self.retry_scheduled = true;
                        return;
                    }
                    self.insert_response_cell(delivery.frame.response_id);
                    self.canonical = delivery.frame.target;
                    self.editor = delivery.frame.target;
                    delivery.visible = true;
                    self.visible_frontiers
                        .insert((delivery.frame.response_id, delivery.frame.generation));
                    self.delivery = Some(delivery);
                    self.phase = Phase::Projected;
                }
                Structure::Invalid | Structure::RepairRequired => {
                    if self.editor == delivery.frame.expected_editor {
                        // Exact live-editor adoption repairs canonical and
                        // re-registers the native frontier. The semantic
                        // response remains pending when it was not yet visible.
                        self.canonical = self.editor;
                        self.replica_epoch = Some(self.controller_epoch);
                        self.delivery = None;
                        self.ipc.clear();
                        self.delayed.clear();
                        self.retained = None;
                        if self.response_cells.contains(&delivery.frame.response_id) {
                            self.pending_responses.remove(&delivery.frame.response_id);
                            self.phase = Phase::Projected;
                        }
                        self.retry_scheduled = true;
                    } else {
                        let response_id = delivery.frame.response_id;
                        let already_visible = self.response_cells.contains(&response_id);
                        let frame = self.make_frame(response_id, Structure::Exact, already_visible);
                        self.retained = Some(frame);
                        self.delivery = None;
                        self.ipc.clear();
                        self.retry_scheduled = true;
                    }
                }
            }
        }

        fn ack(&mut self) {
            let Some(delivery) = self.delivery else {
                return;
            };
            let key = (delivery.frame.response_id, delivery.frame.generation);
            if delivery.frame.structure == Structure::Exact
                && delivery.visible
                && self.editor == delivery.frame.target
                && self.visible_frontiers.contains(&key)
            {
                self.acked_generation = self.acked_generation.max(delivery.frame.generation);
                self.pending_responses.remove(&delivery.frame.response_id);
                if self.retained == Some(delivery.frame) {
                    self.retained = None;
                }
                self.delivery = None;
                self.ipc.retain(|frame| *frame != delivery.frame);
                self.delayed.retain(|frame| *frame != delivery.frame);
                self.retry_scheduled = false;
            } else {
                let mut retry = delivery;
                retry.visible = false;
                self.delivery = Some(retry);
                self.retry_scheduled = true;
            }
        }

        fn commit(&mut self) {
            if self.live_editor
                && self.replica_epoch == Some(self.controller_epoch)
                && self.pending_responses.is_empty()
                && self.retained.is_none()
                && self.delivery.is_none()
                && self.pending_disk.is_none()
                && self.canonical == self.editor
            {
                self.disk = self.editor;
                self.head = self.editor;
                self.disk_queue_items = self.editor_queue_items.clone();
                self.head_queue_items = self.editor_queue_items.clone();
                self.editor_dirty = false;
                self.phase = Phase::Committed;
            }
        }

        fn assert_invariants(&self) {
            let mut unique = BTreeSet::new();
            assert!(
                self.response_cells.iter().all(|id| unique.insert(*id)),
                "duplicate semantic response cell; trace={:?}",
                self.trace
            );
            assert!(
                self.ack_proof_valid,
                "ACK without exact visible proof; trace={:?}",
                self.trace
            );
            assert_eq!(
                self.unsafe_force_disk_writes, 0,
                "force-disk crossed live-editor fence; trace={:?}",
                self.trace
            );
            assert!(
                self.decoded.values().all(|count| *count <= 1),
                "delivery decoded more than once; trace={:?}",
                self.trace
            );
            for (plane, items) in [
                ("canonical", &self.canonical_queue_items),
                ("editor", &self.editor_queue_items),
                ("disk", &self.disk_queue_items),
                ("HEAD", &self.head_queue_items),
            ] {
                assert!(
                    items.values().all(|count| *count <= 1),
                    "duplicate queue item on {plane}; trace={:?}",
                    self.trace,
                );
            }
            if self.live_editor {
                assert!(
                    self.accepted_queue_items
                        .iter()
                        .all(|id| self.editor_queue_items.contains_key(id)),
                    "accepted operator queue item disappeared from live editor; trace={:?}",
                    self.trace,
                );
            }
            if let Some(delivery) = self.delivery
                && delivery.visible
            {
                assert_eq!(
                    self.editor, delivery.frame.target,
                    "visible delivery target differs from editor; trace={:?}",
                    self.trace
                );
            }
            if self.phase == Phase::Committed {
                if self.live_editor {
                    assert_eq!(
                        self.editor, self.canonical,
                        "committed editor/canonical drift; trace={:?}",
                        self.trace
                    );
                }
                assert_eq!(
                    self.disk, self.canonical,
                    "committed disk/canonical drift; trace={:?}",
                    self.trace
                );
                assert_eq!(
                    self.head, self.disk,
                    "committed HEAD/disk drift; trace={:?}",
                    self.trace
                );
                assert_eq!(
                    self.disk_queue_items, self.canonical_queue_items,
                    "committed queue disk/canonical drift; trace={:?}",
                    self.trace,
                );
                assert_eq!(
                    self.head_queue_items, self.disk_queue_items,
                    "committed queue HEAD/disk drift; trace={:?}",
                    self.trace,
                );
                assert!(
                    self.pending_responses.is_empty(),
                    "committed with pending response; trace={:?}",
                    self.trace
                );
            }
        }

        fn stop_faults_and_converge(&mut self) {
            self.fault = Fault::None;
            self.live_editor = true;
            for _ in 0..64 {
                if self.pending_disk.is_some() {
                    // Liveness assumes the environment eventually resolves
                    // the IDE cache-conflict. Saving expresses the stated
                    // editor-buffer authority rule.
                    self.apply(Action::SaveEditor);
                    continue;
                }
                if self.replica_epoch != Some(self.controller_epoch) {
                    self.apply(Action::RegisterReplica);
                    continue;
                }
                if self.retained.is_none()
                    && self.delivery.is_none()
                    && let Some(response_id) = self.pending_responses.iter().next().copied()
                {
                    let already_visible = self.response_cells.contains(&response_id);
                    let frame = self.make_frame(response_id, Structure::Exact, already_visible);
                    self.retained = Some(frame);
                    self.phase = Phase::Captured;
                    continue;
                }
                if self.retained.is_some() && self.delivery.is_none() && self.ipc.is_empty() {
                    self.apply(Action::SendRetained);
                    continue;
                }
                if !self.delayed.is_empty() {
                    self.apply(Action::ReleaseDelayed);
                    continue;
                }
                if self.delivery.is_none() && !self.ipc.is_empty() {
                    self.apply(Action::DeliverOne);
                    continue;
                }
                if let Some(delivery) = self.delivery {
                    self.apply(if delivery.visible {
                        Action::Ack
                    } else {
                        Action::Project
                    });
                    continue;
                }
                self.apply(Action::Commit);
                if self.phase == Phase::Committed {
                    return;
                }
            }
            panic!(
                "world failed to converge after faults stopped; observation={:?} pending={:?} retained={:?} delivery={:?} ipc={} delayed={} live={} retry={} trace={:?}",
                self.observe(),
                self.pending_responses,
                self.retained,
                self.delivery,
                self.ipc.len(),
                self.delayed.len(),
                self.live_editor,
                self.retry_scheduled,
                self.trace,
            );
        }
    }

    #[derive(Debug)]
    struct Rng(u64);

    impl Rng {
        fn new(seed: u64) -> Self {
            Self(seed ^ 0x517c_c1b7_2722_0a95)
        }

        fn next(&mut self, modulo: u64) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            (self.0 >> 32) % modulo
        }

        fn action(&mut self) -> Action {
            match self.next(20) {
                0 => Action::Capture {
                    response_id: self.next(4) as u8,
                    already_visible: self.next(2) == 0,
                    structure: match self.next(3) {
                        0 => Structure::Exact,
                        1 => Structure::Invalid,
                        _ => Structure::RepairRequired,
                    },
                },
                1 => Action::SetFault(match self.next(4) {
                    0 => Fault::None,
                    1 => Fault::DropOnce,
                    2 => Fault::DuplicateOnce,
                    _ => Fault::DelayOnce,
                }),
                2 => Action::SendRetained,
                3 => Action::ReleaseDelayed,
                4 => Action::DeliverOne,
                5 => Action::Project,
                6 => Action::Ack,
                7 => Action::UserEdit,
                8 => Action::AddQueueItem(self.next(4) as u8),
                9 => Action::SaveEditor,
                10 => Action::ExternalDiskChange,
                11 => Action::AcceptPendingDisk,
                12 => Action::ControllerRecycle,
                13 => Action::RegisterReplica,
                14 => Action::Interrupt,
                15 => Action::CloseEditor,
                16 => Action::OpenEditor,
                17 => Action::ForceDisk,
                18 => Action::ResolveAmbiguity(match self.next(6) {
                    0 => AmbiguityEvidence::SameSemanticOperation,
                    1 => AmbiguityEvidence::EditorCausallyNewer,
                    2 => AmbiguityEvidence::ReplicaCausallyNewer,
                    3 => AmbiguityEvidence::ConcurrentCrdtCompatible,
                    4 => AmbiguityEvidence::ConcurrentSemanticConflict,
                    _ => AmbiguityEvidence::MissingCausalProof,
                }),
                _ => Action::Commit,
            }
        }
    }

    fn missing_capabilities(
        actions: &[Action],
        available: &BTreeSet<Capability>,
    ) -> BTreeSet<Capability> {
        actions
            .iter()
            .map(|action| action.capability())
            .filter(|capability| !available.contains(capability))
            .collect()
    }

    #[test]
    fn template_rejection_recovers_from_exact_live_editor_without_ack_wedge() {
        let mut world = World::new();
        world.apply(Action::Capture {
            response_id: 7,
            already_visible: true,
            structure: Structure::Invalid,
        });
        world.apply(Action::SendRetained);
        world.apply(Action::DeliverOne);
        world.apply(Action::Project);

        assert!(world.delivery.is_none());
        assert!(world.retained.is_none());
        assert_eq!(world.response_cells, vec![7]);
        assert_eq!(world.replica_epoch, Some(world.controller_epoch));
        world.apply(Action::Commit);
        assert_eq!(world.phase, Phase::Committed);
    }

    #[test]
    fn duplicate_delivery_with_pending_ack_decodes_once_and_commits_once() {
        let mut world = World::new();
        world.apply(Action::Capture {
            response_id: 2,
            already_visible: false,
            structure: Structure::Exact,
        });
        world.apply(Action::SetFault(Fault::DuplicateOnce));
        world.apply(Action::SendRetained);
        world.apply(Action::DeliverOne);
        world.apply(Action::Project);
        world.apply(Action::DeliverOne);
        world.apply(Action::Ack);
        world.apply(Action::Commit);

        assert_eq!(world.decoded.values().copied().sum::<usize>(), 1);
        assert_eq!(world.response_cells, vec![2]);
        assert_eq!(world.phase, Phase::Committed);
    }

    #[test]
    fn unsaved_queue_item_survives_remote_delivery_and_save_is_idempotent() {
        let mut world = World::new();
        world.apply(Action::AddQueueItem(9));
        world.apply(Action::Capture {
            response_id: 2,
            already_visible: false,
            structure: Structure::Exact,
        });
        world.apply(Action::SendRetained);
        world.apply(Action::DeliverOne);
        world.apply(Action::Project);
        world.apply(Action::Ack);
        world.apply(Action::SaveEditor);
        world.apply(Action::SaveEditor);
        world.apply(Action::Commit);

        assert_eq!(world.editor_queue_items.get(&9), Some(&1));
        assert_eq!(world.canonical_queue_items.get(&9), Some(&1));
        assert_eq!(world.disk_queue_items.get(&9), Some(&1));
        assert_eq!(world.head_queue_items.get(&9), Some(&1));
    }

    #[test]
    fn multiple_pre_run_queue_additions_survive_stale_maintenance_and_converge() {
        let mut world = World::new();
        for item_id in [7, 8, 9] {
            world.apply(Action::AddQueueItem(item_id));
        }
        let live_before = world.editor_queue_items.clone();

        world.apply(Action::RunQueueMaintenance(
            QueueMaintenanceObservation::StaleFirstAdditionOnly,
        ));

        assert_eq!(
            world.editor_queue_items, live_before,
            "stale maintenance must not keep only the first operator addition"
        );
        assert_eq!(
            world.queue_recovery_artifacts,
            vec![QueueRecoveryArtifact {
                reason: "head_advanced_retry_from_live_head",
                preserved_live_items: BTreeSet::from([7, 8, 9]),
            }],
            "the retry must retain a recoverable description of the live queue"
        );

        world.apply(Action::RunQueueMaintenance(
            QueueMaintenanceObservation::LiveHead,
        ));
        world.apply(Action::SaveEditor);
        world.apply(Action::Commit);

        for plane in [
            &world.editor_queue_items,
            &world.canonical_queue_items,
            &world.disk_queue_items,
            &world.head_queue_items,
        ] {
            assert_eq!(plane, &BTreeMap::from([(7, 1), (8, 1), (9, 1)]));
        }
        assert_eq!(world.phase, Phase::Committed);
    }

    #[test]
    fn external_disk_candidate_waits_for_editor_and_save_flush_clears_it() {
        let mut world = World::new();
        let editor_before = world.editor;
        let canonical_before = world.canonical;

        world.apply(Action::ExternalDiskChange);
        assert_eq!(world.editor, editor_before);
        assert_eq!(world.canonical, canonical_before);
        assert_eq!(world.pending_disk, Some(world.disk));

        world.apply(Action::SaveEditor);
        assert_eq!(world.disk, world.editor);
        assert_eq!(world.canonical, world.editor);
        assert_eq!(world.pending_disk, None);

        world.apply(Action::ExternalDiskChange);
        world.apply(Action::AcceptPendingDisk);
        assert_eq!(world.editor, world.disk);
        assert_eq!(world.canonical, world.editor);
        assert_eq!(world.pending_disk, None);

        world.apply(Action::ExternalDiskChange);
        world.apply(Action::UserEdit);
        assert_eq!(world.pending_disk, None);

        world.apply(Action::ExternalDiskChange);
        let disk = world.disk;
        world.apply(Action::CloseEditor);
        assert_eq!(world.pending_disk, None);
        assert_eq!(world.canonical, disk);
    }

    #[test]
    fn accepting_disk_conflict_records_explicit_replacement_of_unsaved_queue_item() {
        let mut world = World::new();
        world.apply(Action::AddQueueItem(9));
        world.apply(Action::ExternalDiskChange);
        world.apply(Action::AcceptPendingDisk);

        assert!(!world.editor_queue_items.contains_key(&9));
        assert!(!world.accepted_queue_items.contains(&9));
        assert_eq!(world.pending_disk, None);
    }

    #[test]
    fn ambiguity_rules_resolve_causal_cases_and_stop_on_semantic_conflicts() {
        assert_eq!(
            resolve_ambiguity(AmbiguityEvidence::SameSemanticOperation),
            AmbiguityResolution::Dedupe
        );
        assert_eq!(
            resolve_ambiguity(AmbiguityEvidence::EditorCausallyNewer),
            AmbiguityResolution::ChooseEditor
        );
        assert_eq!(
            resolve_ambiguity(AmbiguityEvidence::ReplicaCausallyNewer),
            AmbiguityResolution::ChooseReplica
        );
        assert_eq!(
            resolve_ambiguity(AmbiguityEvidence::ConcurrentCrdtCompatible),
            AmbiguityResolution::MergeCrdt
        );
        for evidence in [
            AmbiguityEvidence::ConcurrentSemanticConflict,
            AmbiguityEvidence::MissingCausalProof,
        ] {
            assert_eq!(
                resolve_ambiguity(evidence),
                AmbiguityResolution::NeedsOperator
            );
        }
    }

    #[test]
    fn generated_realtime_ipc_cycle_fault_traces_preserve_safety_and_converge() {
        for seed in 0..512 {
            let mut rng = Rng::new(seed);
            let mut world = World::new();
            for _ in 0..48 {
                world.apply(rng.action());
            }
            world.stop_faults_and_converge();
            world.assert_invariants();
            assert_eq!(
                world.phase,
                Phase::Committed,
                "seed={seed} observation={:?} trace={:?}",
                world.observe(),
                world.trace
            );
        }
    }

    #[test]
    fn lazily_adapter_contract_reports_typed_capability_gaps() {
        let trace = [
            Action::Capture {
                response_id: 1,
                already_visible: false,
                structure: Structure::Exact,
            },
            Action::SetFault(Fault::DelayOnce),
            Action::SendRetained,
            Action::DeliverOne,
            Action::Project,
            Action::Ack,
            Action::ControllerRecycle,
            Action::RegisterReplica,
            Action::Interrupt,
            Action::ResolveAmbiguity(AmbiguityEvidence::ConcurrentSemanticConflict),
            Action::ExternalDiskChange,
            Action::AcceptPendingDisk,
            Action::Commit,
        ];
        let available = BTreeSet::from([
            Capability::RetainedDelivery,
            Capability::SemanticIdempotence,
        ]);
        assert_eq!(
            missing_capabilities(&trace, &available),
            BTreeSet::from([
                Capability::AuthorityEpochFence,
                Capability::BoundedRetry,
                Capability::CancellationPropagation,
                Capability::DurableEffectBarrier,
                Capability::EvidenceBasedAmbiguityResolution,
                Capability::KeyedSingleFlight,
                Capability::PendingExternalDiskDecision,
            ])
        );
    }
}

/// `#implementlazilyintent` reference model: post-registration replay may
/// project an intent and queue an editor effect, but retirement, supersession,
/// or loss of the backing editor model must invalidate that effect before EDT
/// mutation.
mod post_registration_editor_effect_model {
    #[derive(Clone, Copy, Debug)]
    enum Action {
        ProjectIntent,
        QueueEditorEffect,
        RetireIntent,
        SupersedeIntent,
        LoseEditorModel,
        RunEdtEffect,
    }

    const ACTIONS: [Action; 6] = [
        Action::ProjectIntent,
        Action::QueueEditorEffect,
        Action::RetireIntent,
        Action::SupersedeIntent,
        Action::LoseEditorModel,
        Action::RunEdtEffect,
    ];

    #[derive(Clone, Copy, Debug)]
    struct EffectToken {
        intent_generation: u64,
        endpoint_generation: u64,
    }

    #[derive(Clone, Debug, Default)]
    struct Model {
        intent_generation: u64,
        endpoint_generation: u64,
        live_intent: Option<u64>,
        editor_model_backed: bool,
        queued_effect: Option<EffectToken>,
        editor_mutations: usize,
        refused_effects: usize,
        stale_mutation: bool,
    }

    impl Model {
        fn step(&mut self, action: Action) {
            match action {
                Action::ProjectIntent => {
                    self.intent_generation += 1;
                    self.endpoint_generation += 1;
                    self.live_intent = Some(self.intent_generation);
                    self.editor_model_backed = true;
                }
                Action::QueueEditorEffect => {
                    if let Some(intent_generation) = self.live_intent
                        && self.editor_model_backed
                    {
                        self.queued_effect = Some(EffectToken {
                            intent_generation,
                            endpoint_generation: self.endpoint_generation,
                        });
                    }
                }
                Action::RetireIntent => {
                    self.intent_generation += 1;
                    self.live_intent = None;
                }
                Action::SupersedeIntent => {
                    self.intent_generation += 1;
                    self.live_intent = Some(self.intent_generation);
                }
                Action::LoseEditorModel => {
                    self.endpoint_generation += 1;
                    self.editor_model_backed = false;
                }
                Action::RunEdtEffect => {
                    let Some(token) = self.queued_effect.take() else {
                        return;
                    };
                    let current = self.live_intent == Some(token.intent_generation)
                        && self.endpoint_generation == token.endpoint_generation
                        && self.editor_model_backed;
                    let mutations_before = self.editor_mutations;
                    if current {
                        self.editor_mutations += 1;
                    } else {
                        self.refused_effects += 1;
                    }
                    self.stale_mutation |= !current && self.editor_mutations > mutations_before;
                }
            }
        }
    }

    #[test]
    fn intent_retired_while_edt_effect_is_queued_is_refused() {
        for retirement in [
            Action::RetireIntent,
            Action::SupersedeIntent,
            Action::LoseEditorModel,
        ] {
            let mut model = Model::default();
            model.step(Action::ProjectIntent);
            model.step(Action::QueueEditorEffect);
            model.step(retirement);
            model.step(Action::RunEdtEffect);

            assert_eq!(model.editor_mutations, 0, "{retirement:?}");
            assert_eq!(model.refused_effects, 1, "{retirement:?}");
            assert!(!model.stale_mutation, "{retirement:?}");
        }
    }

    #[test]
    fn generated_post_registration_effect_schedules_never_apply_a_stale_token() {
        fn visit(depth: usize, model: Model) {
            if depth == 0 {
                assert!(!model.stale_mutation);
                return;
            }
            for action in ACTIONS {
                let mut next = model.clone();
                next.step(action);
                assert!(!next.stale_mutation, "action={action:?} model={next:?}");
                visit(depth - 1, next);
            }
        }

        visit(6, Model::default());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum FaultPoint {
    SnapshotSave,
    FallbackPatchWrite,
    IpcDelivery,
    TemplateMerge,
    WorkingTreeWrite,
    IndexUpdate,
    GitCommit,
    PostCommitBoundaryReposition,
    SessionCheck,
}

impl FaultPoint {
    const ALL: [FaultPoint; 9] = [
        FaultPoint::SnapshotSave,
        FaultPoint::FallbackPatchWrite,
        FaultPoint::IpcDelivery,
        FaultPoint::TemplateMerge,
        FaultPoint::WorkingTreeWrite,
        FaultPoint::IndexUpdate,
        FaultPoint::GitCommit,
        FaultPoint::PostCommitBoundaryReposition,
        FaultPoint::SessionCheck,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum HarnessKind {
    Codex,
    ClaudeCode,
    OpenCode,
}

impl HarnessKind {
    const ALL: [HarnessKind; 3] = [
        HarnessKind::Codex,
        HarnessKind::ClaudeCode,
        HarnessKind::OpenCode,
    ];

    const fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::ClaudeCode => "claude-code",
            Self::OpenCode => "opencode",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum HarnessMatrixEdge {
    ResponseCapture,
    PendingBacklogMutation,
    QueueConsumption,
    IpcWrite,
    PtyFallbackWrite,
    HookStop,
    HookContinue,
    FailedCloseoutRecovery,
}

impl HarnessMatrixEdge {
    const ALL: [HarnessMatrixEdge; 8] = [
        HarnessMatrixEdge::ResponseCapture,
        HarnessMatrixEdge::PendingBacklogMutation,
        HarnessMatrixEdge::QueueConsumption,
        HarnessMatrixEdge::IpcWrite,
        HarnessMatrixEdge::PtyFallbackWrite,
        HarnessMatrixEdge::HookStop,
        HarnessMatrixEdge::HookContinue,
        HarnessMatrixEdge::FailedCloseoutRecovery,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HookOutcome {
    StopFinalAnswer,
    ContinueInOwnerPane,
    ContinueViaAutoLoop,
    ContinueManually,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SimCommand {
    EditPrompt,
    EditLaterPrompt,
    AddMalformedBacklogItem,
    CaptureResponse,
    CaptureFallbackResponse,
    ApplyCapturedResponse,
    Commit,
    FailCommit,
    RepairBoundary,
    DuplicateVisibleResponse,
    CrashAt(FaultPoint),
    PostCommitIpcRepositionSignal,
    Recover,
    SessionClear,
    SessionRestart,
    SessionRestartForce,
    SessionRestartForcePreInterruptIdle,
    BindRouteOwner,
    SupervisorReady,
    SupervisorBusy,
    SupervisorWaitingInput,
    SupervisorBlocked,
    SupervisorClosed,
    DispatchRoutePrompt,
    /// `#qflood`: an explicit operator dispatch (JB `Run Agent Doc`), never
    /// coalesced. Driven only by targeted tests, not the random generator, so the
    /// seed corpus traces are unchanged.
    DispatchOperatorPrompt,
    /// `#routeblockux1`: an operator route dispatch found protected prompt input
    /// already drafted in the target pane. The route must fail closed without
    /// creating dispatch/proof churn.
    DispatchOperatorPromptWithProtectedDraft,
    ProveDispatchAccepted,
    StaleSupervisorUpdate,
    ObserveStalePane,
    ObserveMissingPane,
    DriftProjection,
    RepairProjection,
    PromoteStartingPromptReady,
    /// `#jbtsiftnosub`: model the JB `Run Agent Doc` auto-start dispatch path. The
    /// re-verify gate sends only when the freshly created pane shows a harness
    /// dispatch-ready prompt; while the actor is still `Starting` (cold-starting
    /// composer, input-accepting but not yet submit-ready) it must fail closed and
    /// record `dispatch_into_starting_pane` instead of typing into a not-ready
    /// composer. Driven only by targeted tests, not the random generator, so the
    /// seed corpus traces are unchanged.
    DispatchAutoStartRoutePrompt,
    /// `#runexitrestart`: model the supervisor idle-watch queue-drain dispatch
    /// AFTER a session restart. A freshly-restarted pane's actor is still
    /// `Starting` (the harness composer is coming up — a prompt glyph may render
    /// from the edge-triggered pty buffer but it is not yet submit-ready). The
    /// drain gate must re-verify a fresh-capture dispatch-ready prompt before
    /// injecting the `agent-doc <FILE>` trigger; while still `Starting` it must
    /// fail closed and record `dispatch_into_restarting_pane` rather than typing
    /// (and re-typing each idle tick → the operator-observed duplicate triggers
    /// with no submit). Once the dispatch-ready prompt is observed
    /// (`PromoteStartingPromptReady`) the same drain dispatches once. Driven only
    /// by targeted tests, not the random generator, so seed corpus traces are
    /// unchanged.
    DispatchIdleQueueDrainAfterRestart,
    /// `#jbdisprecycle`: mark the project supervisor mid-`execve` recycle (the
    /// lib-install auto-recycle / operator-restart hot-reload window). Models the
    /// project-scoped `recycle_inflight` marker `idle_watch.rs` writes before each
    /// reexec; while set, a `route` dispatch must defer (NOT type) so a trigger is
    /// never injected across the boundary where the submit Enter is dropped.
    MarkSupervisorRecycleInflight,
    /// `#jbdisprecycle`: the fresh post-recycle supervisor settled onto the new
    /// binary and cleared the recycle-inflight marker (or the TTL expired).
    SettleSupervisorRecycle,
    /// `#jbdisprecycle` R4: a JB `Run Agent Doc` dispatch that lands during the
    /// recycle window. While `recycle_inflight` is set it must fail closed and
    /// record `dispatch_into_recycling_pane` (no inject, no re-type); once the
    /// recycle settles (`SettleSupervisorRecycle`) the same dispatch injects the
    /// trigger exactly once. Proves R1+R3's defer-until-settle + submit-once.
    DispatchDuringSupervisorRecycle,
    BusyInterruptRecoveryReady,
    RepairBusyProjectionWithReadyPrompt,
    AdminPauseQueue,
    AdminPauseQueueStale,
    AdminResumeQueue,
    AdminDrainQueue,
    AdminHandoff,
    AdminHandoffStale,
    AdminReap,
    AdminReapStale,
    SupervisorHeartbeatReattach,
    SupervisorHeartbeatStale,
    SyncProtectedGrowthManual,
    SyncProtectedGrowthPassive,
    SyncProtectedGrowthFocusVisible,
    SyncDetachableReplaceManual,
    SyncDetachableReplacePassive,
    SyncVisibleFocusPreserve,
    SyncRerequestVisibleEditorManual,
    SyncRerequestVisibleEditorPassive,
    /// `#resyncreactivecardinality`: registry repair has completed without
    /// moving panes. Re-publish the editor's exact two-document projection as
    /// one reactive resync generation. Driven only by the targeted regression.
    SyncReactiveResyncGeneration,
    SyncFocusStashedMoveBeforeSelect,
    /// A passive editor reconciliation observes both a stale pane in the stash
    /// and an inactive document pane beside the operator. The attached client
    /// must preserve its window and pane throughout, not merely restore them.
    SyncPassiveSelectionPreservesClientFocus,
    /// `#exact-visible-focus-swap`: a deliberate editor tab/focus change emits
    /// `sync --focus <file> --exact-visible --no-autostart`. The focused document's
    /// owned pane is alive but parked off-screen and still proves ownership, so the
    /// reconcile must swap it into view (not preserve the stale layout).
    SyncExactVisibleFocusResolvesOffscreenOwner,
    /// `#exact-visible-focus-swap` (unproven-owner fallback): the focused
    /// document's off-screen pane can NOT prove ownership (e.g. supervisor identity
    /// unavailable mid-recycle), so the exact-visible sync falls back to the safe
    /// layout-preserving behavior instead of borrowing/swapping a pane.
    SyncExactVisibleFocusUnprovenPreserve,
    /// `#recyclerestart-agent`: a killed/recycling pane left a manual
    /// `Sync Tmux Layout` subprocess holding the plugin sync guard.
    StartBlockingSyncAfterKillPane,
    /// Advance the simulated clock past the production sync stale-holder bound.
    AdvanceSyncGuardBeyondBound,
    /// Manual `Sync Tmux Layout` click through the production FFI acquire decision.
    SyncLayoutClick,
    /// The sync subprocess exits normally and releases the plugin-local guard.
    FinishSyncLayout,
    /// `#clearcontresume` recycle + clear pipeline. Driven only by targeted
    /// tests, not the random generator, so the seed corpus traces are unchanged.
    ///
    /// A go-mode `queue_active: true` head is waiting to drain.
    ActivateGoModeQueueHead,
    /// `#qfocsup`: a `[focused-cycle]` head is active. The in-session loop must
    /// yield it, then the supervisor clears and re-dispatches it into a fresh agent.
    ActivateFocusedCycleQueueHead,
    /// `#qfocsup`: model session-check emitting the supervisor-drain yield outcome
    /// for the active focused-cycle head.
    SessionCheckFocusedCycleDeferredForSupervisorDrain,
    /// `#qfocsup`: the freshly dispatched agent has drained the active head and
    /// materialized a response.
    FreshAgentDrainActiveHead,
    /// Model controller restart recovery over durable open dispatch receipts. The
    /// recovery marker is keyed by receipt identity, so replaying recovery updates
    /// the same marker instead of appending one marker per restart.
    RecoverControllerDispatchMarkers,
    /// A later `cargo install` made the supervisor's launch binary stale.
    MarkSupervisorBinaryStale,
    /// `#midturn-recycle-resume`: set whether an agent-doc cycle is OPEN — preflight
    /// taken, finalize not yet committed, or an IPC ack connection in flight. While
    /// open, every recycle / restart-reexec arm must DEFER (`DeferCycleOpen`) so the
    /// `execve` cannot sever the in-flight finalize IPC listener mid-cycle and drive
    /// the next finalize into `live_prompt_drift_after_preflight`.
    SetAgentDocCycleOpen(bool),
    /// `#midturn-recycle-resume` Phase B: model a fresh supervisor BOOT from an
    /// `execve` recycle. Drives the pure `boot_resume_action` over the modeled
    /// open-cycle / child-survived / consumed state and applies the outcome —
    /// re-dispatching the interrupted turn (and latching the consume) only when the
    /// cycle is open AND the child died AND it was not already consumed.
    SupervisorRecycleBoot,
    /// `#supselfheal` Phase 2 (`#supselfheal-wedgetrigger`): the editor-IPC write
    /// path is wedged — the converge closeout has refused repeated writes against a
    /// nominally-active JB listener (the de-wedge latch tripped `degraded`). The
    /// wedge feeds `supervisor_recycle_action` as `write_wedged`, forcing an
    /// immediate recycle of a stale supervisor even when auto-recycle is opted OUT.
    MarkWriteWedged,
    /// `#midturn-wedge-recycle`: toggle whether a supervisor IPC connection is being
    /// handled right now. While `true` the current tick is NOT a safe intra-turn
    /// checkpoint, so a wedge recycle defers; the first tick where it is `false`
    /// recycles (mirrors idle_watch.rs's `inflight_connection_handlers()` gate).
    MarkIpcInflight(bool),
    /// `#suprecyclestall`: the next in-place `execve` recycle will fail to launch
    /// any candidate binary (e.g. an old pre-fix launch path). The watch must fall
    /// back to continuing on the current binary, never `process::exit` (which would
    /// orphan the child and hang the pane).
    MarkReexecWillFail,
    /// `AGENT_DOC_SUPERVISOR_AUTO_RECYCLE` opt-in is enabled. (`#supselfheal` made
    /// this the default, so this is now redundant with the baseline; kept for tests
    /// that assert the explicit opt-in path.)
    EnableSupervisorAutoRecycle,
    /// `#supselfheal`: explicit opt OUT of turn-boundary self-recycle (a falsey
    /// `AGENT_DOC_SUPERVISOR_AUTO_RECYCLE` / frontmatter / project knob). A stale
    /// supervisor then only surfaces staleness (`Detect`) instead of self-recycling.
    DisableSupervisorAutoRecycle,
    /// Operator `admin recycle --all-projects`: mark this supervisor to recycle
    /// at the next idle boundary.
    OperatorRecycleMark,
    /// An operator-deferred clear is still pending delivery.
    DeferOperatorClearPending,
    /// One supervisor idle-queue-watch poll. Drives the production cooldown
    /// resume, recycle, and drain decision predicates exactly as `idle_watch.rs`
    /// does, with no live pane.
    SupervisorIdleQueueTick,
    /// `#qflood2`: the idle-queue watch sent its OWN `/clear` between queue items
    /// (an opt-in context reset or a `/clear` head). Engages the in-memory settle
    /// gate so the next drain trigger is not injected into the in-flight clear.
    SupervisorContextResetClear,
    /// `#qflood2`: set whether the routed trigger is already pending in the
    /// modeled composer (the live pane-capture dedup signal).
    SetTriggerAlreadyPending(bool),
    /// `#qdedup`: queue a supervisor/CP fresh-context handoff request. Targeted
    /// tests repeat this while the pane is busy to prove the inter-turn stage
    /// delivers one de-duplicated `/clear` + `agent-doc <FILE>` set.
    QueueBetweenTurnFreshContextHandoff,
    /// `#supkill-bg`: an explicit operator `restart-supervisor` (IPC `Restart`). The
    /// next idle tick drives the `supervisor_restart_action` drain-and-supersede
    /// policy: drain the in-flight turn, then in-place `execve` reexec (stale) or
    /// relaunch (fresh) at the turn boundary.
    RequestSupervisorRestart,
    /// `#actorswitchdefer`: the operator edited the document frontmatter `agent:`
    /// from `from` to `to` while an actor of the OLD harness is live. Records the
    /// launch harness (the authoritative actor's stored harness) and the new
    /// frontmatter-resolved harness so the route defer + idle-watch restart flow can
    /// be driven offline against the real production predicates. Driven only by
    /// targeted tests, not the random generator.
    SwitchFrontmatterHarness {
        from: &'static str,
        to: &'static str,
    },
    /// `#actorswitchdefer`: a `route` dispatch (JB `Run Agent Doc`) lands while the
    /// frontmatter harness no longer matches the live actor's harness. Drives the
    /// production `mismatched_authoritative_actor_can_be_replaced` guard + the route
    /// `agent_change_restart_enabled` bail: a healthy old-harness actor must DEFER
    /// (not replace the pane); a disabled knob must bail explicitly.
    DispatchRouteAfterHarnessSwitch,
    /// `#actorswitchdefer`: opt OUT of agent-change-restart (`#agentreloadrestart`
    /// knob off). With it off the idle-watch never restarts on a harness change, so
    /// the route defer would never self-heal.
    DisableAgentChangeRestart,
    /// `#actorswitchdefer`: one supervisor idle-watch tick that runs the production
    /// `agent_change_restart_decision` boundary gate for a pending harness switch.
    /// At a quiet dispatch-ready boundary it emits `harness_change_detected` +
    /// `agent_restart_triggered`; mid-turn / paused it emits only the detection +
    /// holds the switch pending (no silent drop).
    SupervisorHarnessSwitchTick,
    /// `#actorswitchdefer`: the supervisor restart loop (run.rs) re-read the changed
    /// frontmatter, re-resolved the harness, and respawned the new harness FRESH
    /// (`agent_restart_performed`), completing the deferred switch.
    PerformDeferredHarnessRestart,
    /// `#actorharnessrecordwriteback`: turn OFF the persisted-record half of the
    /// harness-switch writeback, reproducing the shipped regression where the restart
    /// updated only the supervisor's in-memory harness identity.
    DisableActorHarnessRecordWriteback,
    /// `#actorharnessswitchcoverage`: a `route` dispatch landing AFTER the
    /// harness-switch restart already completed. With the persisted record corrected
    /// (and both sides of the comparison normalized) this must be ACCEPTED; with the
    /// stale record it reproduces the "defers forever to a restart that already ran"
    /// bug.
    DispatchRouteAfterHarnessRestart,
    /// `#supdead-coldstart-fallback`: the supervisor PROCESS dies abruptly (crash /
    /// OOM / host reboot) leaving a stale socket FILE on disk. Drives the actor to
    /// `Dead` and the socket to `StaleRefused` (`connect()` → ECONNREFUSED), the
    /// pre-condition for the dead-supervisor recovery decision. Driven only by
    /// targeted tests, not the random generator, so the seed corpus traces are
    /// unchanged.
    AbandonSupervisorToDeadSocket,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SupervisorLifecycle {
    Starting,
    Ready,
    Busy,
    WaitingInput,
    Blocked,
    Closed,
    /// `#supdead-coldstart-fallback`: the supervisor PROCESS is gone (crash / OOM /
    /// host reboot) but a stale socket FILE still lingers on disk, so a `connect()`
    /// is actively refused (`ECONNREFUSED`). Distinct from `Closed` (an orderly
    /// shutdown that reaped its socket): a `Dead` supervisor cannot be restarted /
    /// recycled in place — the in-place IPC connect hits the stale socket and fails,
    /// so recovery must reap the stale socket and COLD-START a fresh supervisor
    /// (unless a safe cold-start is impossible, in which case it surfaces actionable
    /// guidance instead of a raw ECONNREFUSED).
    Dead,
}

/// `#supdead-coldstart-fallback`: connect-liveness of the supervisor's AF_UNIX
/// socket file, modeled so a scenario can assert "socket present but not
/// accepting" independently of the lifecycle. Maps onto the production
/// [`agent_doc_supervisor_io::ipc::SocketLiveness`] (Live vs Dead)
/// that `probe_socket` returns, but keeps a third `Absent` state so the model can
/// distinguish a reaped socket from a stale-but-present one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SupervisorSocket {
    /// `connect()` succeeds — a live supervisor process is listening.
    Live,
    /// The socket file is present on disk but the owning process is gone, so
    /// `connect()` is actively refused (`ECONNREFUSED`). The stale-socket case.
    StaleRefused,
    /// No socket file on disk (never started, or reaped during cold-start).
    Absent,
}

impl SupervisorSocket {
    /// Map to the production `SocketLiveness` the live `probe_socket` returns: both
    /// a refused stale socket and a missing socket classify as `Dead`.
    fn liveness(self) -> agent_doc_supervisor_io::ipc::SocketLiveness {
        use agent_doc_supervisor_io::ipc::SocketLiveness;
        match self {
            Self::Live => SocketLiveness::Live,
            Self::StaleRefused | Self::Absent => SocketLiveness::Dead,
        }
    }

    /// The dead-supervisor `socket_dead` input fed to the production recovery
    /// decision (`probe_socket(...) == SocketLiveness::Dead`).
    fn is_dead(self) -> bool {
        matches!(
            self.liveness(),
            agent_doc_supervisor_io::ipc::SocketLiveness::Dead
        )
    }

    /// The scenario assertion surface: the socket file is present on disk but is
    /// not accepting connections (a stale socket left by a dead process).
    fn present_but_not_accepting(self) -> bool {
        matches!(self, Self::StaleRefused)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueueControlState {
    Resumed,
    Paused,
    Draining,
}

impl QueueControlState {
    const fn as_failed_stage(self, lifecycle: SupervisorLifecycle) -> Option<&'static str> {
        match (self, lifecycle) {
            (Self::Paused, _) => Some("queue_paused"),
            (Self::Draining, SupervisorLifecycle::Ready) => None,
            (Self::Draining, _) => Some("actor_busy_draining"),
            (Self::Resumed, _) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RestartInterruptOutcome {
    StillBusy,
    IdleBeforeForceKill,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActorState {
    generation: u64,
    session_id: String,
    pane_id: Option<String>,
    lifecycle: SupervisorLifecycle,
}

impl ActorState {
    fn initial() -> Self {
        Self {
            generation: 1,
            session_id: "session-1".to_string(),
            pane_id: Some("%1".to_string()),
            lifecycle: SupervisorLifecycle::Starting,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DispatchReceipt {
    generation: u64,
    session_id: String,
    pane_id: String,
    proved: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyncMode {
    Full,
    SafePassive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SyncOutcome {
    PreservedLayoutAndFocused,
    ReplacedDetachable(usize),
    AttachedAroundProtected(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FullContentReplacementSource {
    CompactExchange,
    FullContentRepair,
    IpcTimeoutRecovery,
}

impl FullContentReplacementSource {
    const ALL: [FullContentReplacementSource; 3] = [
        FullContentReplacementSource::CompactExchange,
        FullContentReplacementSource::FullContentRepair,
        FullContentReplacementSource::IpcTimeoutRecovery,
    ];

    const fn as_str(self) -> &'static str {
        match self {
            Self::CompactExchange => "compact_exchange",
            Self::FullContentRepair => "full_content_repair",
            Self::IpcTimeoutRecovery => "ipc_timeout_recovery",
        }
    }
}

#[derive(Debug, Clone)]
struct SyncProjection {
    visible: Vec<String>,
    protected_open_cycle: BTreeSet<String>,
    stashed: BTreeSet<String>,
    active: Option<String>,
}

impl SyncProjection {
    fn protected_growth_case() -> Self {
        Self {
            visible: vec!["protected".to_string(), "sibling".to_string()],
            protected_open_cycle: BTreeSet::from(["protected".to_string()]),
            stashed: BTreeSet::from(["requested".to_string()]),
            active: Some("sibling".to_string()),
        }
    }

    fn detachable_replacement_case() -> Self {
        Self {
            visible: vec!["protected".to_string(), "detachable".to_string()],
            protected_open_cycle: BTreeSet::from(["protected".to_string()]),
            stashed: BTreeSet::from(["requested".to_string()]),
            active: Some("protected".to_string()),
        }
    }

    /// The editor document is already visible under one pane. This models the
    /// state in which a duplicate-claim / pane-id churn makes sync re-request the
    /// same document that is already on screen (see
    /// `duplicate_live_pane_claim` in `agent-doc-orchestration::sync`). The
    /// reconciler must recognize the document as already present and must NOT
    /// attach a second editor pane for it (no-duplicate-editor-pane invariant).
    fn rerequested_visible_editor_case() -> Self {
        Self {
            visible: vec!["editor".to_string()],
            protected_open_cycle: BTreeSet::new(),
            stashed: BTreeSet::new(),
            active: Some("editor".to_string()),
        }
    }

    fn reactive_resync_case() -> Self {
        Self {
            visible: vec!["agent-doc-bugs2".to_string(), "haiven-dev".to_string()],
            protected_open_cycle: BTreeSet::new(),
            stashed: BTreeSet::from([
                "lazily".to_string(),
                "tsift".to_string(),
                "equity".to_string(),
                "haiven-models".to_string(),
            ]),
            active: Some("haiven-dev".to_string()),
        }
    }

    fn apply_requested_projection(
        &mut self,
        requested_docs: &[&str],
        focus_doc: &str,
        mode: SyncMode,
    ) -> SyncOutcome {
        let _mode_label = match mode {
            SyncMode::Full => "manual",
            SyncMode::SafePassive => "safe-passive",
        };
        let requested: BTreeSet<String> = requested_docs
            .iter()
            .map(|doc| (*doc).to_string())
            .collect();
        let visible: BTreeSet<String> = self.visible.iter().cloned().collect();
        let missing: Vec<String> = requested
            .iter()
            .filter(|doc| !visible.contains(*doc))
            .cloned()
            .collect();
        let detachable_unwanted: Vec<usize> = self
            .visible
            .iter()
            .enumerate()
            .filter_map(|(index, doc)| {
                if requested.contains(doc) || self.protected_open_cycle.contains(doc) {
                    None
                } else {
                    Some(index)
                }
            })
            .collect();

        if missing.len() > detachable_unwanted.len() {
            let attach_count = missing.len();
            for doc in missing {
                self.stashed.remove(&doc);
                self.visible.push(doc);
            }
            if self.visible.iter().any(|doc| doc == focus_doc) {
                self.active = Some(focus_doc.to_string());
            }
            return SyncOutcome::AttachedAroundProtected(attach_count);
        }

        let replacement_count = missing.len();
        for (doc, index) in missing.into_iter().zip(detachable_unwanted) {
            self.stashed.remove(&doc);
            self.stashed.insert(self.visible[index].clone());
            self.visible[index] = doc;
        }
        if self.visible.iter().any(|doc| doc == focus_doc) {
            self.active = Some(focus_doc.to_string());
        }
        SyncOutcome::ReplacedDetachable(replacement_count)
    }

    /// Move-before-select focus (`#tmux-switch-lag`). The passive fast-handoff
    /// path must surface a stashed actor pane *out* of the stash window before
    /// selecting it, so the doc-to-doc switch never shows an intermediate stash
    /// frame. Model that ordering: when the focus target is stashed, perform the
    /// move (remove from `stashed`, add to `visible`) FIRST, then the select (set
    /// `active`). Selecting a still-stashed pane is the bug — the
    /// `active`-not-in-`stashed` structural invariant catches it.
    fn focus_doc_move_before_select(&mut self, doc: &str) {
        if self.stashed.remove(doc) {
            // Move: promote the pane into the working layout before selecting.
            if !self.visible.iter().any(|d| d == doc) {
                self.visible.push(doc.to_string());
            }
        }
        // Select only after the move has surfaced the pane.
        self.active = Some(doc.to_string());
    }

    /// The focus target is parked in the stash window. Used by the move-before-
    /// select coverage to prove the switch promotes before selecting.
    fn stashed_focus_case() -> Self {
        Self {
            visible: vec!["agent-doc".to_string()],
            protected_open_cycle: BTreeSet::new(),
            stashed: BTreeSet::from(["requested".to_string()]),
            active: Some("agent-doc".to_string()),
        }
    }

    /// `#exact-visible-focus-swap`: a deliberate editor tab/focus change emits
    /// `sync --focus <file> --exact-visible --no-autostart`. The focused document
    /// (`focused`) owns a LIVE pane that is currently off-screen (parked in a stash
    /// window), while another document (`onscreen`) holds the visible pane. The
    /// exact-visible path must resolve the off-screen owned pane so the reconcile
    /// swaps it into view — not leave the old pane in place.
    fn exact_visible_offscreen_owner_case() -> Self {
        Self {
            visible: vec!["onscreen".to_string()],
            protected_open_cycle: BTreeSet::new(),
            stashed: BTreeSet::from(["focused".to_string()]),
            active: Some("onscreen".to_string()),
        }
    }
}

impl Default for SyncProjection {
    fn default() -> Self {
        Self::protected_growth_case()
    }
}

#[derive(Debug, Clone)]
struct SyncGuardModel {
    locked: bool,
    acquired_at_ms: u64,
    now_ms: u64,
    stale_bound_ms: u64,
    killed_pane_path: bool,
}

impl Default for SyncGuardModel {
    fn default() -> Self {
        Self {
            locked: false,
            acquired_at_ms: 0,
            now_ms: 1,
            stale_bound_ms: agent_doc_sync::DEFAULT_SYNC_LOCK_STALE_BOUND_MS,
            killed_pane_path: false,
        }
    }
}

#[derive(Debug, Clone)]
struct RouteModel {
    durable: ActorState,
    projection: ActorState,
    pending_dispatch: Option<DispatchReceipt>,
    starting_timeout: Option<(u64, String)>,
    queue_control: QueueControlState,
    recovery_marker_keys: BTreeSet<String>,
    supervisor_lease_generation: Option<u64>,
    /// `#jbdisprecycle`: the project supervisor is mid-`execve` recycle right now
    /// (lib-install auto-recycle / operator restart). Models the project-scoped
    /// `recycle_inflight` marker the live `route` dispatch reads before typing.
    recycle_inflight: bool,
    /// `#supdead-coldstart-fallback`: connect-liveness of the supervisor socket
    /// file. A live supervisor binds a `Live` socket; an abandoned (crashed) one
    /// leaves a `StaleRefused` socket; a cold-start reaps it back to `Absent` then
    /// the fresh process binds a new `Live` socket.
    socket: SupervisorSocket,
}

impl RouteModel {
    fn new() -> Self {
        let durable = ActorState::initial();
        Self {
            projection: durable.clone(),
            durable,
            pending_dispatch: None,
            starting_timeout: None,
            queue_control: QueueControlState::Resumed,
            recovery_marker_keys: BTreeSet::new(),
            supervisor_lease_generation: Some(1),
            recycle_inflight: false,
            socket: SupervisorSocket::Live,
        }
    }
}

/// Models the operator **recycle + clear pipeline** (`#clearcontresume`) so the
/// SimWorld engine can drive the SAME production decision predicates the live
/// supervisor idle-queue watch uses — `clear_cooldown_resume_ready` and
/// `supervisor_recycle_action` in `agent_doc_supervisor::lifecycle` —
/// instead of reimplementing the policy in the test harness. The operator's
/// pipeline is `admin recycle --all-projects` (mark recycle at next idle
/// boundary) → `session clear` (record the manual clear-cooldown projection) → the cleared
/// pane settles to a fresh idle prompt → the cooldown auto-expires and the
/// go-mode queue drain resumes as a continuation *step* (not a stall).
#[derive(Debug, Clone, Default)]
struct RecycleClearModel {
    /// Manual clear cooldown is active: an operator `session clear` /
    /// JB `Clear Exchange` recorded the clear-cooldown projection.
    clear_cooldown_active: bool,
    /// Consecutive idle-prompt polls observed since the clear settled, mirroring
    /// `idle_watch.rs`'s `clear_cooldown_idle_ticks` debounce counter.
    clear_cooldown_idle_ticks: u32,
    /// An operator-deferred clear is still pending delivery. That path owns its
    /// own resume, so the cooldown auto-expiry defers to it.
    deferred_operator_clear_pending: bool,
    /// A go-mode `queue_active: true` head is waiting to drain.
    queue_active_head: Option<String>,
    /// The operator marked this supervisor to recycle at the next idle boundary
    /// (`admin recycle --all-projects`).
    operator_recycle_marked: bool,
    /// The supervisor's launch binary is stale (a later `cargo install`), so the
    /// auto-recycle predicate can fire at a turn boundary.
    binary_stale: bool,
    /// Auto-recycle opt-in (`AGENT_DOC_SUPERVISOR_AUTO_RECYCLE`).
    auto_recycle: bool,
    /// `#suprecyclestall`: the next in-place `execve` recycle will fail (e.g. a
    /// launch path that no longer resolves, an old pre-fix supervisor). Models the
    /// `supervisor_perform_reexec` Err path.
    reexec_will_fail: bool,
    /// `#suprecyclestall`: a self-`execve` recycle already failed, so the watch
    /// disabled further recycle attempts and keeps running on the current binary
    /// (mirrors idle_watch.rs's `reexec_recycle_disabled`).
    recycle_disabled: bool,
    /// `#supselfheal` Phase 2 (`#supselfheal-wedgetrigger`): the editor-IPC write is
    /// wedged against a nominally-active listener (the persisted `degraded` latch the
    /// converge closeout reads). Feeds `write_wedged` to `supervisor_recycle_action`.
    write_wedged: bool,
    /// `#midturn-wedge-recycle`: a supervisor IPC connection is being handled right
    /// now, so this tick is NOT a safe intra-turn checkpoint — an `execve` recycle
    /// would sever the in-flight apply. Mirrors idle_watch.rs's
    /// `inflight_connection_handlers() != 0` gate: a wedge recycle waits for the first
    /// tick where this is false before firing.
    ipc_inflight: bool,
    /// `#supselfheal` Phase 3 (`#supselfheal-reexecescalate`): a prior in-place
    /// `execve` recycle failed, so the recycle policy escalates to a bounded
    /// kill+relaunch (`EscalateKillRelaunch`) instead of looping
    /// `continue_current_binary` (mirrors idle_watch.rs's `reexec_failed`).
    reexec_failed: bool,
    /// `#supselfheal` Phase 3: how many bounded kill+relaunch escalations have fired
    /// this supervisor lifetime (mirrors idle_watch.rs's `reexec_escalation_attempts`).
    reexec_escalation_attempts: u32,
    /// `#midturn-recycle-resume`: an agent-doc cycle is OPEN — preflight taken,
    /// finalize not yet committed, or an IPC ack connection in flight (mirrors
    /// idle_watch.rs's `cycle_open` computed from `cycle_state::is_open()` +
    /// `agent_doc_ipc_io::inflight_connection_handlers()`). While true, every recycle /
    /// restart-reexec arm must DEFER so the `execve` cannot sever the in-flight IPC
    /// listener mid-cycle and drive the next finalize into
    /// `live_prompt_drift_after_preflight`.
    cycle_open: bool,
    /// `#midturn-recycle-resume` Phase B: consecutive idle ticks the recycle has been
    /// deferred for an open cycle at a turn boundary (mirrors idle_watch.rs's
    /// `cycle_open_defer_streak`). Once it reaches `MAX_CYCLE_OPEN_DEFER_TICKS` the
    /// watch ESCALATES and forces the recycle, so a never-closing / wedged cycle
    /// cannot starve the stale-binary self-recycle indefinitely.
    cycle_open_defer_streak: u32,
    /// `#midturn-recycle-resume` Phase B: the harness child has died across the
    /// recycle window (it crashed/was killed, or an escalation forced the recycle
    /// over a wedged cycle). A boot reading the still-open `#durablerecycle`
    /// checkpoint must actively re-dispatch the interrupted turn instead of adopting
    /// a (dead) surviving child. Default false: the child normally survives the
    /// `execve` recycle and owns its own resume.
    recycle_child_died: bool,
    /// `#midturn-recycle-resume` Phase B: a prior boot already re-dispatched this
    /// open checkpoint's interrupted turn (mirrors the persisted
    /// `recycle_resume_consumed` latch). A second boot must not re-dispatch again.
    recycle_resume_consumed: bool,
    /// The head the idle-queue watch last injected a trigger for (drain dedup).
    last_dispatched: Option<String>,
    /// `#qflood2`: the watch sent its OWN `/clear` (an opt-in context reset or a
    /// `/clear` queue head) and must hold the next drain trigger until the pane
    /// settles, so the trigger is not injected into the in-flight clear.
    awaiting_clear_settle: bool,
    /// Consecutive idle-prompt polls observed since the watch's own `/clear`,
    /// mirroring `idle_watch.rs`'s `clear_settle_idle_ticks` debounce counter.
    clear_settle_idle_ticks: u32,
    /// `#qflood2`: the routed trigger is already pending/visible in the modeled
    /// composer, so a re-send would stack a duplicate (`recent_lines_contain_trigger`).
    trigger_already_pending: bool,
    /// `#qdedup`: raw between-turn command requests buffered while a turn is
    /// active. The idle boundary composes these as a set.
    between_turn_enqueue: Vec<String>,
    /// `#supkill-bg`: an explicit `restart-supervisor` (IPC `Restart`) is pending. The
    /// idle tick drives the production `supervisor_restart_action` drain-and-supersede
    /// policy — it DRAINS while a turn is in flight (`turn_active`) and only at the
    /// turn boundary re-execs in place (stale binary) or relaunches (fresh binary).
    restart_requested: bool,
    /// `#suprehotreload-agent`: a JB Run Agent Doc style operator dispatch is
    /// waiting for the next stale-binary recycle boundary to prove whether the
    /// cycle observed the fresh binary or mapped to the existing recycle-failure
    /// operator-verify buckets.
    jb_run_recycle_probe_pending: bool,
    /// `#actorswitchdefer`: the harness this supervisor LAUNCHED with (the
    /// authoritative actor's stored harness, e.g. `codex`). Empty when no
    /// harness-switch scenario is active.
    launch_harness: String,
    /// `#actorswitchdefer`: the harness the CURRENT frontmatter `agent:` now
    /// resolves to (e.g. `opencode`). Empty when no harness-switch scenario is
    /// active. A non-empty value differing from `launch_harness` is a pending
    /// switch that route must DEFER (not replace the live pane) and the idle-watch
    /// must drive to a fresh restart.
    frontmatter_harness: String,
    /// `#actorswitchdefer`: agent-change-restart knob (`#agentreloadrestart`). When
    /// false the idle-watch never restarts on a harness change, so the route defer
    /// would never self-heal — route must bail explicitly. Default ON.
    agent_change_restart_enabled: bool,
    /// `#actorharnessrecordwriteback`: the harness stored on the PERSISTED
    /// authoritative actor record — the value `route` actually reads. Deliberately
    /// separate from `launch_harness` (the supervisor's in-memory identity), because
    /// the live bug was exactly that the two diverged: the restart swapped the child
    /// and updated only the in-memory half, so the record kept saying `codex` and
    /// route deferred a switch that had already completed. `transition_state_*`
    /// carries the stored harness forward, so nothing else corrects it.
    persisted_actor_harness: String,
    /// `#actorharnessrecordwriteback`: whether the restart path writes the switched
    /// harness back to the persisted record. Always ON in production; a test can turn
    /// it off to re-prove the original defer-forever regression.
    actor_harness_record_writeback_enabled: bool,
}

#[derive(Debug, Default)]
struct Coverage {
    unresolved_prompt_blocks: usize,
    malformed_backlog_blocks: usize,
    uncommitted_response_blocks: usize,
    duplicate_patchback_blocks: usize,
    boundary_repairs: usize,
    fault_fail_closed: usize,
    fault_recoveries: usize,
    fault_noops: usize,
    fault_points_hit: BTreeSet<FaultPoint>,
    route_generation_rebinds: usize,
    supervisor_lifecycle_updates: usize,
    route_dispatch_acceptances: usize,
    route_dispatch_proofs: usize,
    route_dispatch_coalesced: usize,
    protected_prompt_route_blocks: usize,
    /// `#rdypoll` (§D / img_52): count of REAL trigger injections into the harness
    /// composer. Mirrors the production `dispatch_inject attempt=N` ops.log marker
    /// so a multi-inject regression (the ~7 stacked un-submitted triggers after a
    /// restart) is provable from logs: a correct dispatch injects exactly once
    /// (`attempt=1`), never `attempt=2+`.
    dispatch_injects: usize,
    session_clears: usize,
    deferred_clear_duplicate_suppressed: usize,
    session_restart_busy_refusals: usize,
    session_restart_force_used: usize,
    session_restart_busy_pre_interrupt_idle: usize,
    session_restart_busy_force_killed: usize,
    session_restarts: usize,
    starting_dispatch_blocks: usize,
    starting_timeout_records: usize,
    starting_timeout_coalesces: usize,
    prompt_duplicate_repairs: usize,
    normalization_repair_patches: usize,
    recovery_projection_normalization_divergences: usize,
    stale_source_buffer_skips: usize,
    reconnect_reread_decisions: usize,
    editorless_disk_fallbacks: usize,
    ipc_snapshot_live_prompt_blocks: usize,
    live_prompt_forward_merges: usize,
    already_applied_response_recoveries: usize,
    ack_projection_only_repairs: usize,
    ack_projection_only_blocks: usize,
    visible_duplicate_repairs: usize,
    post_commit_follow_up_handoffs: usize,
    starting_prompt_promotions: usize,
    /// `#jbtsiftnosub`: the JB auto-start dispatch re-verify gate refused to send
    /// into a freshly created pane whose harness was still cold-starting (no
    /// dispatch-ready prompt), recording `dispatch_into_starting_pane`.
    auto_start_starting_pane_blocks: usize,
    /// `#runexitrestart`: the supervisor idle-watch drain gate refused to inject
    /// the `agent-doc <FILE>` trigger into a freshly-RESTARTED pane whose harness
    /// was still coming up (a prompt glyph visible but not yet submit-ready),
    /// recording `dispatch_into_restarting_pane` instead of typing — so a not-ready
    /// restarted composer never accumulates duplicate un-submitted triggers.
    drain_into_restarting_pane_blocks: usize,
    /// `#jbdisprecycle` R4: a `route` dispatch refused to inject the trigger while
    /// the project supervisor was mid-`execve` recycle, recording
    /// `dispatch_into_recycling_pane` instead of typing across the hot-reload
    /// boundary (where the submit Enter is dropped).
    dispatch_into_recycling_pane_blocks: usize,
    busy_dispatch_blocks: usize,
    closed_dispatch_blocks: usize,
    busy_interrupt_recoveries: usize,
    busy_projection_ready_repairs: usize,
    queue_pauses: usize,
    queue_resumes: usize,
    queue_drains: usize,
    queue_paused_dispatch_blocks: usize,
    actor_busy_draining_blocks: usize,
    queue_backpressure_events: usize,
    admin_handoffs: usize,
    admin_reaps: usize,
    supervisor_heartbeat_reattaches: usize,
    supervisor_heartbeat_stale_blocks: usize,
    stale_generation_blocks: usize,
    stale_pane_blocks: usize,
    missing_pane_blocks: usize,
    projection_drift_blocks: usize,
    projection_repairs: usize,
    sync_preserve_layout_blocks: usize,
    sync_detachable_replacements: usize,
    sync_protected_expansions: usize,
    sync_focus_handoffs: usize,
    harness_matrix_edges: BTreeSet<(HarnessKind, HarnessMatrixEdge)>,
    commits: usize,
    post_commit_worktree_checks: usize,
    sync_move_before_select_focuses: usize,
    /// `#clearcontresume`: a lingering manual clear cooldown auto-expired and the
    /// active go-mode queue drain resumed via the production
    /// `clear_cooldown_resume_ready` predicate.
    clear_cooldown_resumes: usize,
    /// The supervisor recycled in place onto a fresh binary at an idle boundary
    /// (operator `admin recycle` or the `supervisor_recycle_action` predicate),
    /// preserving the live pane via `execve`.
    supervisor_recycles: usize,
    /// `#midturn-recycle-resume` Phase B: a never-closing / wedged open cycle deferred
    /// the recycle past `MAX_CYCLE_OPEN_DEFER_TICKS`, so the watch ESCALATED and forced
    /// the recycle (the open cycle can no longer starve a stale-binary self-recycle).
    cycle_open_defer_escalations: usize,
    /// `#midturn-recycle-resume` Phase B: a fresh supervisor boot re-dispatched a
    /// genuinely-interrupted turn from the still-open `#durablerecycle` checkpoint
    /// (the harness child died across the recycle).
    recycle_resume_redispatches: usize,
    /// `#midturn-recycle-resume` Phase B: a fresh supervisor boot adopted a SURVIVING
    /// harness child without re-dispatching (the common case — idempotency: the child
    /// is still running the interrupted turn).
    recycle_resume_adopt_surviving: usize,
    /// `#suprecyclestall`: a self-`execve` recycle failed and the watch fell back to
    /// continuing on the current binary (never `process::exit`, so the pane survives).
    supervisor_recycle_failures: usize,
    /// `#supselfheal` Phase 2 (`#supselfheal-wedgetrigger`): a stale supervisor with a
    /// wedged editor-IPC write recycled immediately because of the wedge — even though
    /// auto-recycle was opted OUT (the wedge overrides the default-OFF surface-only).
    wedge_triggered_recycles: usize,
    /// `#supselfheal` Phase 3 (`#supselfheal-reexecescalate`): a stale supervisor whose
    /// in-place `execve` failed escalated to a bounded kill+relaunch of the harness
    /// child instead of looping `continue_current_binary` forever.
    reexec_kill_relaunch_escalations: usize,
    /// `#supkill-bg`: an explicit `restart-supervisor` drained its in-flight turn,
    /// then hot-reloaded in place via `execve` at the turn boundary (stale binary,
    /// `supervisor_restart_action` → `ReexecInPlace`), preserving the live pane.
    supervisor_restart_drain_reexecs: usize,
    /// `#supkill-bg`: an explicit `restart-supervisor` on a fresh binary relaunched
    /// the child at the turn boundary (`supervisor_restart_action` → `RelaunchChild`).
    supervisor_restart_relaunches: usize,
    /// A go-mode queue head was dispatched by the idle-queue drain decision after
    /// the recycle + clear settled.
    go_drain_dispatches: usize,
    /// `#qfocsup`: session-check yielded a focused-cycle head to the supervisor
    /// clear-and-continue path with `ui_outcome=deferred_for_supervisor_drain`.
    focused_cycle_supervisor_yields: usize,
    /// `#qfocsup`: the supervisor submitted the context reset required before a
    /// focused-cycle fresh-agent drain.
    focused_cycle_context_resets: usize,
    /// `#qfocsup`: the fresh agent consumed the focused-cycle head and left
    /// response-materialization evidence.
    focused_cycle_fresh_agent_drains: usize,
    /// `#qflood2`: the idle-queue drain was held back because the watch's own
    /// `/clear` had not settled yet (the trigger would have been injected into
    /// the in-flight clear and concatenated as `/clear /agent-doc <FILE>`).
    drain_settle_skips: usize,
    /// `#qflood2`: a drain dispatch was skipped because the routed trigger was
    /// already pending in the composer (de-dup: only one trigger lands).
    drain_dedup_skips: usize,
    /// `#recyclerestart-agent`: a killed-pane sync path was modeled instead of
    /// requiring live JB/manual verification.
    sync_kill_pane_proofs: usize,
    /// `#recyclerestart-agent`: a fresh in-flight sync guard correctly deferred a
    /// later click.
    sync_guard_defers: usize,
    /// `#recyclerestart-agent`: a stale plugin-local sync guard was superseded by
    /// the production FFI stale-holder decision.
    sync_guard_stale_releases: usize,
    /// `#recyclerestart-agent`: a sync holder reached its normal finally/unlock path.
    sync_guard_completions: usize,
    /// `#recyclerestart-agent`: proof that recycle promoted the binary instead of
    /// silently killing the session.
    recycle_binary_promotion_proofs: usize,
    /// `#recyclerestart-agent`: proof that the following clear/drain path re-cleared
    /// or restarted the session before the next queue head.
    recycle_session_reclear_proofs: usize,
    /// `#qdedup`: a buffered between-turn command set was delivered at an idle
    /// boundary.
    between_turn_enqueue_deliveries: usize,
    /// `#qdedup`: duplicate raw between-turn command requests suppressed while
    /// composing the delivered set.
    between_turn_enqueue_deduped: usize,
    /// `#qdedup`: pending between-turn commands were held because the pane was
    /// not yet at an idle boundary.
    between_turn_enqueue_busy_skips: usize,
    /// `#suprehotreload-agent`: a JB Run Agent Doc style cycle reached the
    /// stale-binary recycle boundary and observed the fresh binary after promotion.
    suprehot_jb_observed_promotions: usize,
    /// `#suprehotreload-agent`: a JB Run Agent Doc style cycle hit the
    /// stale-binary recycle failure path and mapped to the existing
    /// #recyclerestart-verify/#aazp/#4myd operator-verify proof bucket.
    suprehot_jb_mapped_recycle_failures: usize,
    /// `#smsim` (document_cell_merge Phase 5): an operator↔agent concurrent edit on
    /// DISJOINT nodes auto-merged through `document_cell_merge` — both sides applied,
    /// no ack, no content loss.
    document_cell_merge_node_disjoint: usize,
    /// `#smsim`: a same-node operator↔agent conflict resolved operator-wins via
    /// `document_cell_merge` (operator content in the merged doc).
    document_cell_merge_operator_wins: usize,
    /// `#smsim`: the operator deleted an agent-edited/struck node — the deletion
    /// stood and `document_cell_merge` raised an ack for the next turn.
    document_cell_merge_delete_acks: usize,
    /// `#smsim`: turn-active-area gating — a same-node conflict OUTSIDE the
    /// active `exchange` area auto-resolved operator-wins with NO ack noise, while
    /// the identical conflict INSIDE the active area raised an ack.
    document_cell_merge_scope_gated_acks: usize,
    /// `#harnesshotrebind`: a route harness-switch found a healthy old-harness
    /// authority and accepted a non-dispatchable boundary handoff instead of
    /// replacing or injecting the live pane.
    actor_switch_route_handoffs_accepted: usize,
    /// `#actorswitchdefer`: the supervisor idle-watch detected the frontmatter
    /// harness change (`harness_change_detected`).
    actor_switch_changes_detected: usize,
    /// `#actorswitchdefer`: the idle-watch requested a FRESH restart at a quiet
    /// dispatch-ready boundary (`agent_restart_triggered`).
    actor_switch_restarts_triggered: usize,
    /// `#actorswitchdefer`: the supervisor respawned the new harness fresh
    /// (`agent_restart_performed`), completing the deferred switch.
    actor_switch_restarts_performed: usize,
    /// `#harnesshotrebind`: a paused supervisor held an accepted handoff pending
    /// with queue resume — not manual restart — as the exact prerequisite.
    actor_switch_queue_resume_holds: usize,
    /// `#actorswitchdefer` Part B: route bailed EXPLICITLY because
    /// `agent_change_restart` was disabled (the defer would never self-heal).
    actor_switch_restart_disabled_bails: usize,
    /// `#actorharnessrecordwriteback`: a completed harness-switch restart persisted
    /// the new harness onto the authoritative actor record.
    actor_harness_record_writebacks: usize,
    /// `#actorharnessswitchcoverage`: a `route` dispatch AFTER a completed
    /// harness-switch restart was accepted — no harness-mismatch defer. This is the
    /// half that used to fail silently: the in-memory writeback test passed while
    /// route kept deferring off the stale persisted record.
    actor_switch_post_restart_dispatches: usize,
    /// `#supdead-coldstart-fallback`: the supervisor process was abandoned, leaving
    /// a stale socket (`Dead` lifecycle + `StaleRefused` socket).
    supervisor_deaths: usize,
    /// `#supdead-coldstart-fallback`: a dead supervisor's recovery decision resolved
    /// to ColdStart — the stale socket was reaped and a fresh supervisor cold-started
    /// through the route path.
    dead_supervisor_cold_starts: usize,
    /// `#supdead-coldstart-fallback`: a dead supervisor's recovery decision resolved
    /// to Guidance — an unsafe cold-start was refused (own-ancestor caller, or no
    /// reachable tmux target) and the actionable message was surfaced instead of a
    /// raw ECONNREFUSED, leaving the supervisor `Dead`.
    dead_supervisor_guidance_refusals: usize,
    /// Controller restart recovery observed an already-keyed open dispatch marker
    /// and updated it instead of appending a duplicate marker row.
    recovery_marker_upsert_dedupes: usize,
}

impl Coverage {
    fn record_harness_matrix_edge(&mut self, harness: HarnessKind, edge: HarnessMatrixEdge) {
        self.harness_matrix_edges.insert((harness, edge));
    }

    fn record_block(&mut self, message: &str) {
        if message.contains("unresolved prompt_target") {
            self.unresolved_prompt_blocks += 1;
        }
        if message.contains("malformed tracked checklist item") {
            self.malformed_backlog_blocks += 1;
        }
        if message.contains("response captured but not committed")
            || message.contains("response write applied but not committed")
        {
            self.uncommitted_response_blocks += 1;
        }
        if message.contains("duplicate response patchback") {
            self.duplicate_patchback_blocks += 1;
        }
        if message.contains("fault point") {
            self.fault_fail_closed += 1;
        }
        if message.contains("stale actor generation") {
            self.stale_generation_blocks += 1;
        }
        if message.contains("stale pane observation") {
            self.stale_pane_blocks += 1;
        }
        if message.contains("missing pane observation") {
            self.missing_pane_blocks += 1;
        }
        if message.contains("projection drift") {
            self.projection_drift_blocks += 1;
        }
        if message.contains("supervisor lifecycle Busy cannot accept route dispatch") {
            self.busy_dispatch_blocks += 1;
        }
        if message.contains("failed_stage=queue_paused") {
            self.queue_paused_dispatch_blocks += 1;
            self.queue_backpressure_events += 1;
        }
        if message.contains("failed_stage=actor_busy_draining") {
            self.actor_busy_draining_blocks += 1;
            self.queue_backpressure_events += 1;
        }
        if message.contains("supervisor lifecycle Starting cannot accept route dispatch") {
            self.starting_dispatch_blocks += 1;
        }
        if message.contains("supervisor lifecycle Closed cannot accept route dispatch") {
            self.closed_dispatch_blocks += 1;
        }
        if message.contains("session_restart refused") {
            self.session_restart_busy_refusals += 1;
        }
        if message.contains("supervisor heartbeat stale generation") {
            self.supervisor_heartbeat_stale_blocks += 1;
        }
    }

    fn merge(&mut self, other: Coverage) {
        self.unresolved_prompt_blocks += other.unresolved_prompt_blocks;
        self.malformed_backlog_blocks += other.malformed_backlog_blocks;
        self.uncommitted_response_blocks += other.uncommitted_response_blocks;
        self.duplicate_patchback_blocks += other.duplicate_patchback_blocks;
        self.boundary_repairs += other.boundary_repairs;
        self.fault_fail_closed += other.fault_fail_closed;
        self.fault_recoveries += other.fault_recoveries;
        self.fault_noops += other.fault_noops;
        self.fault_points_hit.extend(other.fault_points_hit);
        self.route_generation_rebinds += other.route_generation_rebinds;
        self.supervisor_lifecycle_updates += other.supervisor_lifecycle_updates;
        self.route_dispatch_acceptances += other.route_dispatch_acceptances;
        self.route_dispatch_proofs += other.route_dispatch_proofs;
        self.route_dispatch_coalesced += other.route_dispatch_coalesced;
        self.dispatch_injects += other.dispatch_injects;
        self.session_clears += other.session_clears;
        self.deferred_clear_duplicate_suppressed += other.deferred_clear_duplicate_suppressed;
        self.session_restart_busy_refusals += other.session_restart_busy_refusals;
        self.session_restart_force_used += other.session_restart_force_used;
        self.session_restart_busy_pre_interrupt_idle +=
            other.session_restart_busy_pre_interrupt_idle;
        self.session_restart_busy_force_killed += other.session_restart_busy_force_killed;
        self.session_restarts += other.session_restarts;
        self.starting_dispatch_blocks += other.starting_dispatch_blocks;
        self.starting_timeout_records += other.starting_timeout_records;
        self.starting_timeout_coalesces += other.starting_timeout_coalesces;
        self.prompt_duplicate_repairs += other.prompt_duplicate_repairs;
        self.normalization_repair_patches += other.normalization_repair_patches;
        self.recovery_projection_normalization_divergences +=
            other.recovery_projection_normalization_divergences;
        self.stale_source_buffer_skips += other.stale_source_buffer_skips;
        self.reconnect_reread_decisions += other.reconnect_reread_decisions;
        self.editorless_disk_fallbacks += other.editorless_disk_fallbacks;
        self.already_applied_response_recoveries += other.already_applied_response_recoveries;
        self.ack_projection_only_repairs += other.ack_projection_only_repairs;
        self.ack_projection_only_blocks += other.ack_projection_only_blocks;
        self.visible_duplicate_repairs += other.visible_duplicate_repairs;
        self.post_commit_follow_up_handoffs += other.post_commit_follow_up_handoffs;
        self.starting_prompt_promotions += other.starting_prompt_promotions;
        self.auto_start_starting_pane_blocks += other.auto_start_starting_pane_blocks;
        self.drain_into_restarting_pane_blocks += other.drain_into_restarting_pane_blocks;
        self.dispatch_into_recycling_pane_blocks += other.dispatch_into_recycling_pane_blocks;
        self.busy_dispatch_blocks += other.busy_dispatch_blocks;
        self.closed_dispatch_blocks += other.closed_dispatch_blocks;
        self.busy_interrupt_recoveries += other.busy_interrupt_recoveries;
        self.busy_projection_ready_repairs += other.busy_projection_ready_repairs;
        self.queue_pauses += other.queue_pauses;
        self.queue_resumes += other.queue_resumes;
        self.queue_drains += other.queue_drains;
        self.queue_paused_dispatch_blocks += other.queue_paused_dispatch_blocks;
        self.actor_busy_draining_blocks += other.actor_busy_draining_blocks;
        self.queue_backpressure_events += other.queue_backpressure_events;
        self.admin_handoffs += other.admin_handoffs;
        self.admin_reaps += other.admin_reaps;
        self.supervisor_heartbeat_reattaches += other.supervisor_heartbeat_reattaches;
        self.supervisor_heartbeat_stale_blocks += other.supervisor_heartbeat_stale_blocks;
        self.stale_generation_blocks += other.stale_generation_blocks;
        self.stale_pane_blocks += other.stale_pane_blocks;
        self.missing_pane_blocks += other.missing_pane_blocks;
        self.projection_drift_blocks += other.projection_drift_blocks;
        self.projection_repairs += other.projection_repairs;
        self.sync_preserve_layout_blocks += other.sync_preserve_layout_blocks;
        self.sync_detachable_replacements += other.sync_detachable_replacements;
        self.sync_protected_expansions += other.sync_protected_expansions;
        self.sync_focus_handoffs += other.sync_focus_handoffs;
        self.harness_matrix_edges.extend(other.harness_matrix_edges);
        self.commits += other.commits;
        self.post_commit_worktree_checks += other.post_commit_worktree_checks;
        self.sync_move_before_select_focuses += other.sync_move_before_select_focuses;
        self.clear_cooldown_resumes += other.clear_cooldown_resumes;
        self.supervisor_recycles += other.supervisor_recycles;
        self.supervisor_recycle_failures += other.supervisor_recycle_failures;
        self.go_drain_dispatches += other.go_drain_dispatches;
        self.focused_cycle_supervisor_yields += other.focused_cycle_supervisor_yields;
        self.focused_cycle_context_resets += other.focused_cycle_context_resets;
        self.focused_cycle_fresh_agent_drains += other.focused_cycle_fresh_agent_drains;
        self.drain_settle_skips += other.drain_settle_skips;
        self.drain_dedup_skips += other.drain_dedup_skips;
        self.sync_kill_pane_proofs += other.sync_kill_pane_proofs;
        self.sync_guard_defers += other.sync_guard_defers;
        self.sync_guard_stale_releases += other.sync_guard_stale_releases;
        self.sync_guard_completions += other.sync_guard_completions;
        self.recycle_binary_promotion_proofs += other.recycle_binary_promotion_proofs;
        self.recycle_session_reclear_proofs += other.recycle_session_reclear_proofs;
        self.between_turn_enqueue_deliveries += other.between_turn_enqueue_deliveries;
        self.between_turn_enqueue_deduped += other.between_turn_enqueue_deduped;
        self.between_turn_enqueue_busy_skips += other.between_turn_enqueue_busy_skips;
        self.suprehot_jb_observed_promotions += other.suprehot_jb_observed_promotions;
        self.suprehot_jb_mapped_recycle_failures += other.suprehot_jb_mapped_recycle_failures;
        self.document_cell_merge_node_disjoint += other.document_cell_merge_node_disjoint;
        self.document_cell_merge_operator_wins += other.document_cell_merge_operator_wins;
        self.document_cell_merge_delete_acks += other.document_cell_merge_delete_acks;
        self.document_cell_merge_scope_gated_acks += other.document_cell_merge_scope_gated_acks;
        self.actor_switch_route_handoffs_accepted += other.actor_switch_route_handoffs_accepted;
        self.actor_switch_changes_detected += other.actor_switch_changes_detected;
        self.actor_switch_restarts_triggered += other.actor_switch_restarts_triggered;
        self.actor_switch_restarts_performed += other.actor_switch_restarts_performed;
        self.actor_harness_record_writebacks += other.actor_harness_record_writebacks;
        self.actor_switch_post_restart_dispatches += other.actor_switch_post_restart_dispatches;
        self.actor_switch_queue_resume_holds += other.actor_switch_queue_resume_holds;
        self.actor_switch_restart_disabled_bails += other.actor_switch_restart_disabled_bails;
        self.supervisor_deaths += other.supervisor_deaths;
        self.dead_supervisor_cold_starts += other.dead_supervisor_cold_starts;
        self.dead_supervisor_guidance_refusals += other.dead_supervisor_guidance_refusals;
        self.recovery_marker_upsert_dedupes += other.recovery_marker_upsert_dedupes;
    }
}

#[derive(Debug)]
struct SimWorld {
    seed: u64,
    trace: Vec<SimCommand>,
    doc: String,
    snapshot: String,
    phase: CyclePhase,
    captured_response: Option<String>,
    pending_fault: Option<FaultPoint>,
    route: RouteModel,
    recycle_clear: RecycleClearModel,
    sync: SyncProjection,
    sync_guard: SyncGuardModel,
    client_visible_window: String,
    client_window_trace: Vec<String>,
    client_visible_pane: String,
    client_pane_trace: Vec<String>,
    ops_log: Vec<String>,
    next_prompt: usize,
    coverage: Coverage,
}

mod engine;

#[derive(Debug)]
struct DeterministicRng(u64);

impl DeterministicRng {
    fn new(seed: u64) -> Self {
        Self(seed ^ 0x9e37_79b9_7f4a_7c15)
    }

    fn next_usize(&mut self, modulo: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        ((self.0 >> 32) as usize) % modulo
    }
}

fn template_doc(exchange_body: &str) -> String {
    format!(
        "---\nagent_doc_session: sim\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
         ## Exchange\n\n\
         <!-- agent:exchange patch=append -->\n\
         {exchange_body}<!-- agent:boundary:initial -->\n\
         <!-- /agent:exchange -->\n\n\
         ## Pending / Not Built\n\n\
         <!-- agent:backlog -->\n\
         - [ ] [#tigersim] Implement the simulator MVP\n\
         <!-- /agent:backlog -->\n\n\
         <!-- agent:icebox -->\n\
         <!-- /agent:icebox -->\n"
    )
}

/// Normalize a committed document for working-tree==HEAD comparison: collapse the
/// boundary marker id to a stable token and strip ` (HEAD)` heading annotations
/// that the working tree/editor buffer is allowed to carry post-commit while the
/// committed blob does not. Anything left differing is real worktree drift.
fn normalize_committed_worktree(doc: &str) -> String {
    let mut out = String::with_capacity(doc.len());
    let mut rest = doc;
    while let Some(start) = rest.find("<!-- agent:boundary:") {
        out.push_str(&rest[..start]);
        let after = &rest[start..];
        match after.find("-->") {
            Some(end) => {
                out.push_str("<!-- agent:boundary:NORM -->");
                rest = &after[end + 3..];
            }
            None => {
                out.push_str(after);
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out.replace(" (HEAD)", "")
}

fn response_patch(topic: &str) -> String {
    format!(
        "<!-- patch:exchange -->\n### Re: {topic} — gpt-5\n\nImplemented and verified.\n<!-- /patch:exchange -->\n"
    )
}

fn harness_response_patch(harness: HarnessKind, topic: &str) -> String {
    format!(
        "<!-- patch:exchange -->\n### Re: {topic} — gpt-5\n\n{} response captured and verified.\n<!-- /patch:exchange -->\n",
        harness.as_str()
    )
}

fn fallback_response(topic: &str) -> String {
    format!("### Re: {topic} — gpt-5\n\nImplemented and verified through fallback.\n")
}

fn harness_fallback_response(harness: HarnessKind, topic: &str) -> String {
    format!(
        "### Re: {topic} — gpt-5\n\n{} PTY fallback response captured and verified.\n",
        harness.as_str()
    )
}

fn hook_outcome_for(harness: HarnessKind, active_queue: bool) -> HookOutcome {
    if !active_queue {
        return HookOutcome::StopFinalAnswer;
    }

    match harness {
        HarnessKind::Codex => HookOutcome::ContinueInOwnerPane,
        HarnessKind::ClaudeCode => HookOutcome::ContinueViaAutoLoop,
        HarnessKind::OpenCode => HookOutcome::ContinueManually,
    }
}

fn harness_matrix_seed(harness: HarnessKind, edge: HarnessMatrixEdge) -> u64 {
    let harness_offset = match harness {
        HarnessKind::Codex => 0,
        HarnessKind::ClaudeCode => 100,
        HarnessKind::OpenCode => 200,
    };
    let edge_offset = match edge {
        HarnessMatrixEdge::ResponseCapture => 1,
        HarnessMatrixEdge::PendingBacklogMutation => 2,
        HarnessMatrixEdge::QueueConsumption => 3,
        HarnessMatrixEdge::IpcWrite => 4,
        HarnessMatrixEdge::PtyFallbackWrite => 5,
        HarnessMatrixEdge::HookStop => 6,
        HarnessMatrixEdge::HookContinue => 7,
        HarnessMatrixEdge::FailedCloseoutRecovery => 8,
    };
    10_000 + harness_offset + edge_offset
}

fn run_harness_matrix_edge(harness: HarnessKind, edge: HarnessMatrixEdge) -> Result<Coverage> {
    let mut world = SimWorld::new(harness_matrix_seed(harness, edge));

    match edge {
        HarnessMatrixEdge::ResponseCapture => {
            world.apply(SimCommand::EditPrompt)?;
            world.captured_response =
                Some(harness_response_patch(harness, "matrix response capture"));
            world.apply_captured_response()?;
            world.try_commit()?;
            assert!(
                world.doc.contains(&format!(
                    "{} response captured and verified.",
                    harness.as_str()
                )),
                "response body missing for {harness:?}:\n{}",
                world.doc
            );
        }
        HarnessMatrixEdge::PendingBacklogMutation => {
            world.apply(SimCommand::EditPrompt)?;
            world.captured_response =
                Some(harness_response_patch(harness, "matrix pending mutation"));
            world.apply_captured_response()?;
            let next_backlog = format!(
                "{}- [ ] [#matrixfollowup] Follow-up from {} pending mutation\n",
                world.component_content("backlog")?,
                harness.as_str()
            );
            world.replace_component_content("backlog", &next_backlog)?;
            world.try_commit()?;
            assert!(
                world
                    .component_content("backlog")?
                    .contains("Follow-up from"),
                "pending/backlog mutation missing for {harness:?}:\n{}",
                world.doc
            );
        }
        HarnessMatrixEdge::QueueConsumption => {
            world.insert_after_exchange(
                "\n## Queue\n\n<!-- agent:queue auto -->\n- do [#matrixhead]\n- do [#matrixnext]\n<!-- /agent:queue -->\n",
            )?;
            world.snapshot = world.doc.clone();
            world.append_to_exchange("❯ do [#matrixhead]\n")?;
            world.replace_component_content(
                "queue",
                "- ~~do [#matrixhead]~~\n- do [#matrixnext]\n",
            )?;
            world.captured_response = Some(harness_response_patch(harness, "do [#matrixhead]"));
            world.apply_captured_response()?;
            world.try_commit()?;
            let queue = world.component_content("queue")?;
            assert!(
                queue.contains("~~do [#matrixhead]~~"),
                "head not consumed:\n{queue}"
            );
            assert!(
                queue.contains("do [#matrixnext]"),
                "next head missing:\n{queue}"
            );
        }
        HarnessMatrixEdge::IpcWrite => {
            world.apply(SimCommand::EditPrompt)?;
            world.captured_response = Some(harness_response_patch(harness, "matrix ipc write"));
            world.apply_captured_response()?;
            world.try_commit()?;
            assert!(
                world.doc.contains("<!-- agent:boundary:committed -->"),
                "IPC closeout did not cross commit boundary for {harness:?}"
            );
        }
        HarnessMatrixEdge::PtyFallbackWrite => {
            world.apply(SimCommand::EditPrompt)?;
            world.captured_response =
                Some(harness_fallback_response(harness, "matrix pty fallback"));
            world.apply_captured_response()?;
            world.try_commit()?;
            assert!(
                world.doc.contains("PTY fallback response captured"),
                "PTY fallback body missing for {harness:?}:\n{}",
                world.doc
            );
        }
        HarnessMatrixEdge::HookStop => {
            assert_eq!(
                hook_outcome_for(harness, false),
                HookOutcome::StopFinalAnswer,
                "clean {harness:?} closeout should allow final answer"
            );
        }
        HarnessMatrixEdge::HookContinue => {
            let expected = match harness {
                HarnessKind::Codex => HookOutcome::ContinueInOwnerPane,
                HarnessKind::ClaudeCode => HookOutcome::ContinueViaAutoLoop,
                HarnessKind::OpenCode => HookOutcome::ContinueManually,
            };
            assert_eq!(
                hook_outcome_for(harness, true),
                expected,
                "active queue continuation semantics changed for {harness:?}"
            );
        }
        HarnessMatrixEdge::FailedCloseoutRecovery => {
            world.apply(SimCommand::EditPrompt)?;
            world.captured_response =
                Some(harness_response_patch(harness, "matrix failed closeout"));
            world.apply_captured_response()?;
            world.apply(SimCommand::CrashAt(FaultPoint::GitCommit))?;
            let err = world.try_commit().unwrap_err();
            assert!(
                err.to_string().contains("fault point GitCommit"),
                "unexpected failed closeout error for {harness:?}: {err}"
            );
            assert!(
                matches!(world.phase, CyclePhase::Interrupted(FaultPoint::GitCommit)),
                "failed closeout should fail closed before recovery for {harness:?}"
            );
            world.apply(SimCommand::Recover)?;
            assert_eq!(world.phase, CyclePhase::Committed);
            assert_eq!(world.snapshot, world.doc);
        }
    }

    world.coverage.record_harness_matrix_edge(harness, edge);
    world.strict_closeout_invariants()?;
    Ok(world.coverage)
}

fn post_exchange_scratch_comment(prompt: &str) -> String {
    format!(
        "\n###\n\n<!--\n{prompt}\n#spec-test-build-install-commit-push\n---\nUse route, preflight, write, IPC, repair, and compact no-delete coverage.\n-->\n"
    )
}

fn assert_owned_scratch_comment_preserved(doc: &str, prompt: &str) {
    assert!(
        doc.contains(&post_exchange_scratch_comment(prompt)),
        "owned post-exchange scratch comment must survive:\n{doc}"
    );
}

fn setup_baseline_drift_capture(
    seed: u64,
    response: &str,
) -> (
    tempfile::TempDir,
    PathBuf,
    agent_doc_capture_io::CaptureRecord,
    SimWorld,
) {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
    let doc = dir.path().join("doc.md");
    let mut world = SimWorld::new(seed);
    world.apply(SimCommand::EditPrompt).unwrap();
    std::fs::write(&doc, &world.doc).unwrap();
    agent_doc_snapshot_io::checkpoint_document_baseline(
        &doc,
        &world.doc,
        agent_doc_ops_log_io::log_op,
    )
    .unwrap();
    let capture = agent_doc_capture_io::capture_response(&doc, response).unwrap();
    (dir, doc, capture, world)
}

fn apply_response_and_save_current(doc: &Path, world: &mut SimWorld, response: &str) -> Result<()> {
    world.captured_response = Some(response.to_string());
    world.apply_captured_response()?;
    world.apply(SimCommand::Commit)?;
    std::fs::write(doc, &world.doc)?;
    agent_doc_snapshot_io::checkpoint_document_baseline(
        doc,
        &world.doc,
        agent_doc_ops_log_io::log_op,
    )?;
    Ok(())
}

#[derive(Debug)]
struct CorpusRun {
    coverage: Coverage,
    schedules: usize,
    steps: usize,
    elapsed: Duration,
}

impl CorpusRun {
    fn command_count(&self) -> usize {
        self.schedules * self.steps
    }

    fn assert_within_budget(&self, budget: Duration, label: &str) {
        eprintln!(
            "{label}: schedules={} steps={} commands={} elapsed_ms={} budget_ms={}",
            self.schedules,
            self.steps,
            self.command_count(),
            self.elapsed.as_millis(),
            budget.as_millis()
        );
        assert!(
            self.elapsed <= budget,
            "{label} exceeded runtime budget: elapsed={:?} budget={:?}",
            self.elapsed,
            budget
        );
    }
}

#[test]
fn closeout_sim_fixed_seed_corpus_exercises_recent_failure_classes() {
    let run = SimWorld::run_seed_corpus(FAST_CORPUS_SEEDS, FAST_CORPUS_STEPS).unwrap();
    run.assert_within_budget(FAST_CORPUS_BUDGET, "fast simulator corpus");
    let coverage = run.coverage;

    assert!(
        coverage.commits > 0,
        "seed corpus must include valid committed closeouts"
    );
    assert!(
        coverage.boundary_repairs > 0,
        "seed corpus must exercise deterministic boundary repair"
    );
    assert!(
        coverage.unresolved_prompt_blocks > 0,
        "seed corpus must exercise unresolved prompt closeout blocks"
    );
    assert!(
        coverage.malformed_backlog_blocks > 0,
        "seed corpus must exercise malformed backlog closeout blocks"
    );
    assert!(
        coverage.uncommitted_response_blocks > 0,
        "seed corpus must exercise captured/write-applied uncommitted closeout blocks"
    );
    assert!(
        coverage.duplicate_patchback_blocks > 0,
        "seed corpus must exercise duplicate visible response blocks"
    );
    assert!(
        coverage.fault_fail_closed > 0,
        "seed corpus must exercise fault-triggered fail-closed interruptions"
    );
    assert!(
        coverage.fault_recoveries > 0,
        "seed corpus must exercise deterministic fault recovery"
    );
    assert!(
        coverage.fault_noops > 0,
        "seed corpus must exercise invariant-preserving fault no-ops"
    );
    assert_eq!(
        coverage.fault_points_hit.len(),
        FaultPoint::ALL.len(),
        "seed corpus must inject every named closeout fault point"
    );
    assert!(
        coverage.route_generation_rebinds > 0,
        "seed corpus must exercise route owner generation changes"
    );
    assert!(
        coverage.supervisor_lifecycle_updates > 0,
        "seed corpus must exercise supervisor lifecycle facts"
    );
    assert!(
        coverage.route_dispatch_acceptances > 0,
        "seed corpus must exercise accepted route dispatch"
    );
    assert!(
        coverage.route_dispatch_proofs > 0,
        "seed corpus must exercise dispatch proof"
    );
    assert!(
        coverage.busy_dispatch_blocks > 0,
        "seed corpus must block busy/bootstrap route dispatch before current ready proof"
    );
    assert!(
        coverage.queue_pauses > 0
            && coverage.queue_resumes > 0
            && coverage.queue_drains > 0
            && coverage.queue_backpressure_events > 0,
        "seed corpus must exercise controller queue controls and backpressure receipts"
    );
    assert!(
        coverage.actor_busy_draining_blocks > 0 && coverage.queue_paused_dispatch_blocks > 0,
        "seed corpus must block paused queues and busy draining actors before dispatch"
    );
    assert!(
        coverage.admin_handoffs > 0 && coverage.admin_reaps > 0,
        "seed corpus must exercise generation-guarded admin handoff and reap"
    );
    assert!(
        coverage.supervisor_heartbeat_reattaches > 0
            && coverage.supervisor_heartbeat_stale_blocks > 0,
        "seed corpus must exercise supervisor heartbeat reattach and stale heartbeat rejection"
    );
    assert!(
        coverage.stale_generation_blocks > 0,
        "seed corpus must reject stale actor generations"
    );
    assert!(
        coverage.stale_pane_blocks > 0,
        "seed corpus must block stale pane observations"
    );
    assert!(
        coverage.missing_pane_blocks > 0,
        "seed corpus must block missing pane observations"
    );
    assert!(
        coverage.projection_drift_blocks > 0,
        "seed corpus must diagnose projection drift"
    );
    assert!(
        coverage.projection_repairs > 0,
        "seed corpus must repair projection drift from durable actor state"
    );
    assert!(
        coverage.sync_protected_expansions > 0,
        "seed corpus must exercise protected-closeout sync expansion cases"
    );
    assert!(
        coverage.sync_detachable_replacements > 0,
        "seed corpus must exercise detachable-pane sync replacement cases"
    );
    assert!(
        coverage.sync_focus_handoffs > 0,
        "seed corpus must exercise sync focus handoffs"
    );
    assert!(
        coverage.post_commit_worktree_checks > 0,
        "seed corpus must exercise the post-commit IPC reposition working-tree==HEAD guard (#postcommit-ipc-worktree-corruption)"
    );
    assert!(
        coverage.sync_move_before_select_focuses > 0,
        "seed corpus must exercise move-before-select stash focus (#tmux-switch-lag)"
    );
}

#[test]
#[ignore = "run by `make check` as the medium deterministic simulator corpus"]
fn closeout_sim_medium_seed_corpus_runs_wider_deterministic_budget() {
    let run = SimWorld::run_seed_corpus(MEDIUM_CORPUS_SEEDS, MEDIUM_CORPUS_STEPS).unwrap();
    run.assert_within_budget(MEDIUM_CORPUS_BUDGET, "medium simulator corpus");
    let coverage = run.coverage;

    assert!(
        coverage.commits >= 10,
        "medium seed corpus should include many valid committed closeouts"
    );
    assert!(
        coverage.fault_points_hit.len() == FaultPoint::ALL.len(),
        "medium seed corpus must keep every named closeout fault point covered"
    );
    assert!(
        coverage.route_dispatch_acceptances > 0 && coverage.route_dispatch_proofs > 0,
        "medium seed corpus must keep route dispatch/proof coverage"
    );
    assert!(
        coverage.projection_drift_blocks > 0 && coverage.projection_repairs > 0,
        "medium seed corpus must keep projection drift and repair coverage"
    );
    assert!(
        coverage.queue_backpressure_events > 0
            && coverage.admin_handoffs > 0
            && coverage.admin_reaps > 0
            && coverage.supervisor_heartbeat_reattaches > 0,
        "medium seed corpus must keep queue/admin/heartbeat control-plane coverage"
    );
    assert!(
        coverage.sync_protected_expansions > 0 && coverage.sync_detachable_replacements > 0,
        "medium seed corpus must keep sync expansion/replacement coverage"
    );
}

#[test]
fn post_exchange_comment_ownership_sim_covers_cleanup_and_handoff_paths() {
    let prompt = "The post-exchange scratch ownership prompt should not be deleted by duplicate cleanup. #spec-test-build-install-commit-push";
    let file = Path::new("sim.md");
    let mut world = SimWorld::new(9_901);
    world.append_to_exchange(&format!("❯ {prompt}\n")).unwrap();
    world
        .insert_after_exchange(&post_exchange_scratch_comment(prompt))
        .unwrap();
    world.snapshot = world.doc.clone();

    let route_cleaned =
        SimWorld::route_style_duplicate_prompt_cleanup(&world.doc, &[world.doc.as_str()]).unwrap();
    assert_eq!(
        route_cleaned, world.doc,
        "route-style cleanup must treat the visible document as comment ownership proof"
    );

    let preflight_cleaned = SimWorld::route_style_duplicate_prompt_cleanup(
        &world.doc,
        &[world.doc.as_str(), world.snapshot.as_str()],
    )
    .unwrap();
    assert_eq!(
        preflight_cleaned, world.doc,
        "preflight-style recovery must not delete visible scratch comments"
    );

    let direct_write = agent_doc_template_io::normalize_template_structure_or_fail_preserving(
        &world.doc,
        file,
        Some(&world.snapshot),
    )
    .unwrap();
    assert_owned_scratch_comment_preserved(&direct_write, prompt);

    let (ipc_handoff, changed) = agent_doc_write_converge_io::dedupe_ipc_snapshot_content(
        file,
        Some(&world.snapshot),
        &direct_write,
        "sim_ipc",
    )
    .unwrap();
    assert!(
        !changed,
        "IPC/plugin handoff must not rewrite owned scratch comments"
    );
    assert_owned_scratch_comment_preserved(&ipc_handoff, prompt);

    let mut repair_world = world;
    repair_world.captured_response = Some(response_patch("comment ownership"));
    repair_world.apply_captured_response().unwrap();
    let repaired_write = agent_doc_template_io::normalize_template_structure_or_fail_preserving(
        &repair_world.doc,
        file,
        Some(&repair_world.snapshot),
    )
    .unwrap();
    assert_owned_scratch_comment_preserved(&repaired_write, prompt);

    let compacted_exchange = "### Session Summary\n\nCompacted content archived.\n";
    repair_world
        .replace_component_content("exchange", compacted_exchange)
        .unwrap();
    assert_owned_scratch_comment_preserved(&repair_world.doc, prompt);

    let generated = {
        let mut generated_world = SimWorld::new(9_902);
        generated_world
            .append_to_exchange(&format!("❯ {prompt}\n"))
            .unwrap();
        let before = generated_world.doc.clone();
        generated_world
            .insert_after_exchange(&post_exchange_scratch_comment(prompt))
            .unwrap();
        (before, generated_world.doc)
    };
    let (scrubbed, changed) = agent_doc_write_converge_io::dedupe_ipc_snapshot_content(
        file,
        Some(&generated.0),
        &generated.1,
        "sim_generated",
    )
    .unwrap();
    assert!(
        changed,
        "generated duplicate comment residue without ownership proof must still be scrubbed"
    );
    assert!(
        !scrubbed.contains(&format!("<!--\n{prompt}")),
        "generated duplicate prompt text should be removed from post-exchange comment residue:\n{scrubbed}"
    );
    assert!(
        scrubbed
            .contains("Use route, preflight, write, IPC, repair, and compact no-delete coverage."),
        "unrelated scratch lines in generated mixed comments must remain:\n{scrubbed}"
    );
}

#[test]
fn closeout_sim_blocks_later_prompt_after_response_write() {
    let mut world = SimWorld::new(42);
    world.apply(SimCommand::EditPrompt).unwrap();
    world.apply(SimCommand::CaptureResponse).unwrap();
    world.apply(SimCommand::ApplyCapturedResponse).unwrap();
    world.apply(SimCommand::EditLaterPrompt).unwrap();

    let err = world.try_commit().unwrap_err();
    assert!(
        err.to_string().contains("unresolved prompt_target"),
        "unexpected closeout error: {err}"
    );
}

#[test]
fn post_commit_ipc_reposition_signal_keeps_worktree_equal_to_head() {
    // `#postcommit-ipc-worktree-corruption`: drive a clean committed closeout,
    // then fire the post-commit IPC boundary-reposition signal at the working
    // tree. The production reposition is idempotent on an already-clean committed
    // boundary, so the visible file must stay byte-equal to HEAD and the
    // working-tree==HEAD invariant must hold.
    let mut world = SimWorld::new(515);
    world.apply(SimCommand::EditPrompt).unwrap();
    world.apply(SimCommand::CaptureResponse).unwrap();
    world.apply(SimCommand::ApplyCapturedResponse).unwrap();
    world.try_commit().unwrap();
    assert!(matches!(world.phase, CyclePhase::Committed));
    assert_eq!(
        world.doc, world.snapshot,
        "committed closeout must leave working tree == HEAD"
    );

    world
        .apply(SimCommand::PostCommitIpcRepositionSignal)
        .unwrap();
    assert_eq!(
        world.coverage.post_commit_worktree_checks, 1,
        "post-commit reposition signal must record a worktree==HEAD check"
    );
    world.assert_structural_invariants().unwrap();
    assert_eq!(
        normalize_committed_worktree(&world.doc),
        normalize_committed_worktree(&world.snapshot),
        "post-commit IPC reposition signal must not drift the working tree from HEAD"
    );
}

#[test]
fn move_before_select_promotes_stashed_pane_before_focus() {
    // `#tmux-switch-lag`: focusing a doc whose pane is parked in stash must move
    // it out of stash (into visible) BEFORE selecting it, so the switch never
    // shows an intermediate stash frame. The active pane must end up visible and
    // not in stash; the structural invariant rejects a still-stashed selection.
    let mut world = SimWorld::new(733);
    world
        .apply(SimCommand::SyncFocusStashedMoveBeforeSelect)
        .unwrap();
    world.assert_structural_invariants().unwrap();
    assert_eq!(world.sync.active.as_deref(), Some("requested"));
    assert!(
        !world.sync.stashed.contains("requested"),
        "focused pane must be promoted out of the stash window"
    );
    assert!(
        world.sync.visible.iter().any(|d| d == "requested"),
        "promoted pane must be visible in the working layout"
    );
    assert_eq!(world.coverage.sync_move_before_select_focuses, 1);

    // Negative control: a select that leaves the pane in stash (the flash bug)
    // must fail the move-before-select ordering invariant.
    let mut bug = SimWorld::new(734);
    bug.sync = SyncProjection::stashed_focus_case();
    bug.sync.active = Some("requested".to_string()); // selected while still stashed
    let err = bug.assert_structural_invariants().unwrap_err();
    assert!(
        err.to_string().contains("#tmux-switch-lag"),
        "unexpected ordering error: {err}"
    );
}

#[test]
fn passive_selection_never_surfaces_stash_or_adjacent_pane() {
    // Regression for transient stash focus theft: a final-state-only model
    // misses the bug because the reconcile restores the working window after
    // briefly activating a stale background pane in stash. Track every client
    // window transition and require focus neutrality throughout.
    let mut world = SimWorld::new(3_008);
    world
        .apply(SimCommand::SyncPassiveSelectionPreservesClientFocus)
        .unwrap();

    assert_eq!(
        world.client_window_trace,
        vec!["working".to_string()],
        "safe-passive reconciliation must never make the attached client display stash"
    );
    assert_eq!(world.client_visible_window, "working");
    assert_eq!(
        world.client_pane_trace,
        vec!["operator".to_string()],
        "safe-passive reconciliation must never select an adjacent document pane"
    );
    assert_eq!(world.client_visible_pane, "operator");
}

#[test]
fn post_commit_worktree_drift_is_distinguishable_from_head() {
    // Negative control for the working-tree==HEAD guard: the normalized comparison
    // the invariant relies on must distinguish a stale/spliced working-tree buffer
    // (the #postcommit-ipc-worktree-corruption bug) from the committed blob, while
    // still treating `(HEAD)` annotations and boundary-id churn as equal.
    let mut world = SimWorld::new(516);
    world.apply(SimCommand::EditPrompt).unwrap();
    world.apply(SimCommand::CaptureResponse).unwrap();
    world.apply(SimCommand::ApplyCapturedResponse).unwrap();
    world.try_commit().unwrap();
    assert!(matches!(world.phase, CyclePhase::Committed));

    let head = world.snapshot.clone();
    // Allowed post-commit divergence: editor `(HEAD)` annotations + boundary id.
    let annotated = head
        .replace("### Re: sim closeout", "### Re: sim closeout (HEAD)")
        .replace("agent:boundary:committed", "agent:boundary:live-42");
    assert_eq!(
        normalize_committed_worktree(&annotated),
        normalize_committed_worktree(&head),
        "(HEAD)/boundary-id artifacts must normalize equal to HEAD"
    );

    // Real corruption: the IPC listener splices a duplicate response body into the
    // visible file. This must NOT normalize equal to HEAD.
    let corrupted = format!("{head}### Re: sim closeout — gpt-5\n\nStale spliced buffer.\n");
    assert_ne!(
        normalize_committed_worktree(&corrupted),
        normalize_committed_worktree(&head),
        "a spliced working-tree buffer must be detectable as drift from HEAD"
    );
}

#[test]
fn closeout_sim_blocks_malformed_tracked_backlog_line() {
    let mut world = SimWorld::new(7);
    world.apply(SimCommand::EditPrompt).unwrap();
    world.apply(SimCommand::CaptureResponse).unwrap();
    world.apply(SimCommand::ApplyCapturedResponse).unwrap();
    world.apply(SimCommand::AddMalformedBacklogItem).unwrap();

    let err = world.try_commit().unwrap_err();
    assert!(
        err.to_string().contains("malformed tracked checklist item"),
        "unexpected closeout error: {err}"
    );
}

#[test]
fn closeout_sim_requires_captured_response_to_cross_commit_boundary() {
    let mut world = SimWorld::new(11);
    world.apply(SimCommand::EditPrompt).unwrap();
    world.apply(SimCommand::CaptureResponse).unwrap();

    let err = world.strict_closeout_invariants().unwrap_err();
    assert!(
        err.to_string()
            .contains("response captured but not committed"),
        "unexpected closeout error: {err}"
    );
}

#[test]
fn closeout_sim_collapses_boundary_drift_to_single_marker() {
    let mut world = SimWorld::new(99);
    world
        .append_to_exchange("<!-- agent:boundary:stale-one -->\n")
        .unwrap();
    world
        .append_to_exchange("<!-- agent:boundary:stale-two -->\n")
        .unwrap();

    world.apply(SimCommand::RepairBoundary).unwrap();
    world.assert_structural_invariants().unwrap();
    assert!(world.doc.contains("<!-- agent:boundary:sim-boundary -->"));
}

#[test]
fn closeout_sim_fault_points_fail_closed_then_recover() {
    for (index, fault) in FaultPoint::ALL.into_iter().enumerate() {
        let mut world = SimWorld::new(1_000 + index as u64);
        match fault {
            FaultPoint::TemplateMerge | FaultPoint::WorkingTreeWrite => {
                world.apply(SimCommand::CaptureResponse).unwrap();
                world.apply(SimCommand::CrashAt(fault)).unwrap();
                world.apply(SimCommand::ApplyCapturedResponse).unwrap();
            }
            FaultPoint::FallbackPatchWrite => {
                world.apply(SimCommand::CaptureFallbackResponse).unwrap();
                world.apply(SimCommand::CrashAt(fault)).unwrap();
                world.apply(SimCommand::ApplyCapturedResponse).unwrap();
            }
            _ => {
                world.apply(SimCommand::CaptureResponse).unwrap();
                world.apply(SimCommand::ApplyCapturedResponse).unwrap();
                world.apply(SimCommand::CrashAt(fault)).unwrap();
                world.apply(SimCommand::Commit).unwrap();
            }
        }

        if fault == FaultPoint::IpcDelivery {
            assert_eq!(world.phase, CyclePhase::Committed);
            assert!(
                world.coverage.fault_noops > 0,
                "IPC delivery faults should be invariant-preserving no-ops"
            );
        } else {
            assert!(
                matches!(world.phase, CyclePhase::Interrupted(observed) if observed == fault),
                "fault {fault:?} should fail closed before recovery; phase={:?}",
                world.phase
            );
            assert!(
                world.captured_response.is_some(),
                "fault {fault:?} must retain the captured response as recovery authority"
            );
            assert!(
                world.coverage.fault_fail_closed > 0,
                "fault {fault:?} should be recorded as a fail-closed interruption"
            );
            world.apply(SimCommand::Recover).unwrap();
            assert_eq!(
                world.phase,
                CyclePhase::Committed,
                "fault {fault:?} should recover to a committed closeout"
            );
            assert_eq!(world.snapshot, world.doc);
        }
    }
}

#[test]
fn repeated_wedged_post_commit_cycles_never_accumulate_boundaries() {
    let mut world = SimWorld::new(1_099);

    for cycle in 0..8 {
        let stale = format!("<!-- agent:boundary:wedged-{cycle} -->\n");
        world.doc = world.doc.replace(
            "<!-- /agent:exchange -->",
            &(stale + "<!-- /agent:exchange -->"),
        );
        world.snapshot = world.doc.clone();
        world.phase = CyclePhase::WriteApplied;

        world
            .apply(SimCommand::CrashAt(
                FaultPoint::PostCommitBoundaryReposition,
            ))
            .unwrap();
        world.apply(SimCommand::Commit).unwrap();
        assert!(
            matches!(
                world.phase,
                CyclePhase::Interrupted(FaultPoint::PostCommitBoundaryReposition)
            ),
            "cycle {cycle} must model the wedged post-commit handoff"
        );
        assert_eq!(
            world
                .doc
                .matches(agent_doc_element_boundary::boundary::BOUNDARY_PREFIX)
                .count(),
            1,
            "the binary-owned committed projection must already be collapsed before the wedged editor handoff; cycle={cycle}\n{}",
            world.doc
        );

        world.apply(SimCommand::Recover).unwrap();
        assert_eq!(world.phase, CyclePhase::Committed);
        assert_eq!(world.snapshot, world.doc);
        assert_eq!(
            world
                .snapshot
                .matches(agent_doc_element_boundary::boundary::BOUNDARY_PREFIX)
                .count(),
            1,
            "recovery must preserve the singleton boundary; cycle={cycle}\n{}",
            world.snapshot
        );
    }
}

#[test]
fn closeout_sim_harness_matrix_covers_agent_backends_and_edge_classes() {
    let mut observed = BTreeSet::new();

    for harness in HarnessKind::ALL {
        for edge in HarnessMatrixEdge::ALL {
            let coverage = run_harness_matrix_edge(harness, edge)
                .unwrap_or_else(|err| panic!("{harness:?}/{edge:?} failed: {err}"));
            assert!(
                coverage.harness_matrix_edges.contains(&(harness, edge)),
                "{harness:?}/{edge:?} did not record matrix coverage"
            );
            observed.extend(coverage.harness_matrix_edges);
        }
    }

    for harness in HarnessKind::ALL {
        for edge in HarnessMatrixEdge::ALL {
            assert!(
                observed.contains(&(harness, edge)),
                "missing harness matrix coverage for {harness:?}/{edge:?}"
            );
        }
    }
}

#[test]
fn route_sim_accepts_dispatch_only_after_current_supervisor_ready() {
    let mut world = SimWorld::new(2_001);
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(world.coverage.route_dispatch_acceptances, 0);

    world.apply(SimCommand::SupervisorReady).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    world.apply(SimCommand::ProveDispatchAccepted).unwrap();

    assert_eq!(world.coverage.route_dispatch_acceptances, 1);
    assert_eq!(world.coverage.route_dispatch_proofs, 1);
}

#[test]
fn route_sim_promotes_starting_prompt_ready_before_dispatch() {
    let mut world = SimWorld::new(2_004);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(world.coverage.route_dispatch_acceptances, 0);

    world.apply(SimCommand::PromoteStartingPromptReady).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    world.apply(SimCommand::ProveDispatchAccepted).unwrap();

    assert_eq!(world.coverage.starting_prompt_promotions, 1);
    assert_eq!(world.coverage.route_dispatch_acceptances, 1);
    assert_eq!(world.coverage.route_dispatch_proofs, 1);
}

#[test]
fn route_sim_auto_start_dispatch_waits_for_dispatch_ready_prompt_before_send() {
    // #jbtsiftnosub: JB `Run Agent Doc` auto-started a fresh pane/supervisor, typed
    // the `agent-doc <FILE>` trigger into the harness composer, but did NOT submit
    // — a cold-start race where the harness TUI had not reached a dispatch-ready
    // prompt when the trigger was typed. The auto-start dispatch must wait for the
    // harness dispatch-ready prompt and re-verify it immediately before the send;
    // if the send is attempted while the pane is still starting it must fail closed
    // and record `dispatch_into_starting_pane` instead of typing into a
    // not-yet-submit-ready composer.
    let mut world = SimWorld::new(2_005);
    world.apply(SimCommand::BindRouteOwner).unwrap();

    // Cold-start: the fresh pane's actor is still `Starting` (composer accepts
    // input but is not yet submit-ready). The auto-start dispatch must fail closed.
    world
        .apply(SimCommand::DispatchAutoStartRoutePrompt)
        .unwrap();
    assert_eq!(
        world.coverage.auto_start_starting_pane_blocks, 1,
        "auto-start dispatch must fail closed while the harness is still cold-starting"
    );
    assert_eq!(
        world.coverage.route_dispatch_acceptances, 0,
        "the trigger must NOT be sent into a still-starting composer"
    );
    let ops_log = world.ops_log.join("\n");
    assert!(
        ops_log.contains("dispatch_into_starting_pane")
            && ops_log.contains("reason=harness_not_dispatch_ready_before_auto_start_send"),
        "ops log must record dispatch_into_starting_pane for the cold-start race:\n{ops_log}"
    );

    // Once the harness dispatch-ready prompt is observed (the re-verify gate
    // clears), the same auto-start dispatch sends and is proven submitted.
    world.apply(SimCommand::PromoteStartingPromptReady).unwrap();
    world
        .apply(SimCommand::DispatchAutoStartRoutePrompt)
        .unwrap();
    world.apply(SimCommand::ProveDispatchAccepted).unwrap();

    assert_eq!(world.coverage.starting_prompt_promotions, 1);
    assert_eq!(
        world.coverage.route_dispatch_acceptances, 1,
        "after the dispatch-ready prompt is observed the auto-start trigger must submit"
    );
    assert_eq!(world.coverage.route_dispatch_proofs, 1);
    assert_eq!(
        world.coverage.auto_start_starting_pane_blocks, 1,
        "the ready auto-start dispatch must not re-trip the starting-pane block"
    );
    // #jbtsiftnosub / #j9ja: the cleared re-verify gate must log the SUCCESS
    // counterpart so an operator live test is provable from ops.log.
    let ops_log = world.ops_log.join("\n");
    assert!(
        ops_log.contains("auto_start_dispatch_ready_confirmed"),
        "ops log must record auto_start_dispatch_ready_confirmed once the dispatch-ready gate clears:\n{ops_log}"
    );
}

#[test]
fn route_sim_restart_drain_waits_for_dispatch_ready_prompt_before_send() {
    // #runexitrestart: the operator RESTARTED an agent-doc session, then JB
    // `Run Agent Doc` / the supervisor idle-watch drain re-injected the
    // `agent-doc <FILE>` trigger into the freshly-restarted pane while its harness
    // was still coming up — the trigger was typed ~7 times into the composer, none
    // submitted. Distinct from the cold AUTO-START race (#jbtsiftnosub): here the
    // SAME pane is restarted (Starting), and the supervisor idle-watch drain gate
    // (`idle_queue_prompt_visible`) trusted the weak pty-buffer prompt-glyph signal
    // off the `Ready` fast path. The drain must re-verify a fresh-capture
    // dispatch-ready prompt before injecting; while the pane is still restarting it
    // must fail closed, record `dispatch_into_restarting_pane`, and inject NOTHING
    // (so repeated idle ticks cannot stack duplicate un-submitted triggers).
    let mut world = SimWorld::new(2_006);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    // Restart the live session: the same pane drops back to `Starting` (the
    // restarted harness composer is coming up, not yet submit-ready).
    world.apply(SimCommand::SessionRestartForce).unwrap();
    assert_eq!(world.route.durable.lifecycle, SupervisorLifecycle::Starting);

    // The idle-watch drain must fail closed on EVERY tick while the pane is still
    // restarting — and never type the trigger (no duplicate accumulation).
    for _ in 0..7 {
        world
            .apply(SimCommand::DispatchIdleQueueDrainAfterRestart)
            .unwrap();
    }
    assert_eq!(
        world.coverage.drain_into_restarting_pane_blocks, 7,
        "the restart drain must fail closed on every tick while the harness is still restarting"
    );
    assert_eq!(
        world.coverage.route_dispatch_acceptances, 0,
        "the trigger must NOT be sent into a still-restarting composer (no ~7 duplicate triggers)"
    );
    assert_eq!(
        world.coverage.go_drain_dispatches, 0,
        "no drain may dispatch while the restarted pane is not yet dispatch-ready"
    );
    let ops_log = world.ops_log.join("\n");
    assert!(
        ops_log.contains("dispatch_into_restarting_pane")
            && ops_log.contains("reason=harness_not_dispatch_ready_before_restart_drain_send"),
        "ops log must record dispatch_into_restarting_pane for the restart race:\n{ops_log}"
    );
    // `#rdypoll` (§D / img_52): NOTHING was injected while the pane stayed
    // not-ready, so no `dispatch_inject` marker may have been emitted yet.
    assert_eq!(
        world.coverage.dispatch_injects, 0,
        "no trigger may be injected while the restarted pane is not yet dispatch-ready"
    );
    assert!(
        !ops_log.contains("dispatch_inject"),
        "no dispatch_inject marker may be logged while the pane is not-ready:\n{ops_log}"
    );

    // Once the restarted harness reaches a dispatch-ready prompt the same drain
    // sends and is proven submitted, exactly once.
    world.apply(SimCommand::PromoteStartingPromptReady).unwrap();
    world
        .apply(SimCommand::DispatchIdleQueueDrainAfterRestart)
        .unwrap();
    world.apply(SimCommand::ProveDispatchAccepted).unwrap();

    assert_eq!(world.coverage.starting_prompt_promotions, 1);
    assert_eq!(
        world.coverage.route_dispatch_acceptances, 1,
        "after the dispatch-ready prompt is observed the restart drain must submit once"
    );
    assert_eq!(world.coverage.route_dispatch_proofs, 1);
    assert_eq!(
        world.coverage.drain_into_restarting_pane_blocks, 7,
        "the ready restart drain must not re-trip the restarting-pane block"
    );
    // `#rdypoll` (§D / img_52): exactly ONE real injection after ready — ops.log
    // shows a single `dispatch_inject attempt=1`, never N stacked injections.
    assert_eq!(
        world.coverage.dispatch_injects, 1,
        "the ready restart drain must inject the trigger exactly once (no ~7 stacked copies)"
    );
    let ops_log = world.ops_log.join("\n");
    assert!(
        ops_log.contains("dispatch_inject pane=") && ops_log.contains("attempt=1"),
        "ops log must record the single dispatch_inject attempt=1 marker:\n{ops_log}"
    );
    assert!(
        !ops_log.contains("attempt=2"),
        "a correct restart drain must never log a second dispatch_inject attempt:\n{ops_log}"
    );
}

#[test]
fn route_sim_harness_switch_persists_record_so_post_restart_dispatch_does_not_defer() {
    // `#actorharnessswitchcoverage`: the live 2026-07-18 repro, end to end. ops.log
    // showed `agent_restart_performed old_harness=codex new_harness=claude
    // action=spawn_fresh_harness` at 04:12:09 and then, five seconds later,
    // `route_authoritative_actor_harness_mismatch_deferred stored_harness=codex
    // expected_harness=claude-code` — route deferring to a boundary restart that had
    // already run.
    //
    // The pre-existing in-memory coverage
    // (`set_current_harness_updates_state_backbone_harness_identity`) passed
    // throughout, because it only asserted the getter round-trip. This scenario
    // covers the half that actually failed: the PERSISTED record route reads.
    //
    // codex→claude specifically, not codex→opencode: `claude` is the harness whose
    // raw launch binary (`claude`) differs from its normalized name (`claude-code`),
    // so it also pins `#actorharnessnormcompare`.
    let mut world = SimWorld::new(7_104);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();

    world
        .apply(SimCommand::SwitchFrontmatterHarness {
            from: "codex",
            to: "claude",
        })
        .unwrap();
    world
        .apply(SimCommand::SupervisorHarnessSwitchTick)
        .unwrap();
    world
        .apply(SimCommand::PerformDeferredHarnessRestart)
        .unwrap();
    assert_eq!(
        world.coverage.actor_switch_restarts_performed, 1,
        "the deferred codex→claude restart must complete"
    );
    assert_eq!(
        world.coverage.actor_harness_record_writebacks, 1,
        "the completed restart must persist the new harness onto the actor record"
    );
    // Stored NORMALIZED, so route's normalized `expected_harness` compares equal.
    assert_eq!(world.recycle_clear.persisted_actor_harness, "claude-code");

    // The dispatch that used to defer forever.
    world
        .apply(SimCommand::DispatchRouteAfterHarnessRestart)
        .unwrap();
    assert_eq!(
        world.coverage.actor_switch_post_restart_dispatches, 1,
        "a dispatch after the completed switch must be ACCEPTED"
    );
    assert_eq!(
        world.coverage.actor_switch_route_handoffs_accepted, 0,
        "route must not queue a handoff after the boundary restart already ran"
    );
    let ops_log = world.ops_log.join("\n");
    assert!(
        ops_log.contains("actor_harness_record_writeback harness=claude-code"),
        "ops log must record the persisted writeback:\n{ops_log}"
    );
    assert!(
        !ops_log.contains("route_authoritative_actor_harness_mismatch_deferred"),
        "no harness-mismatch defer may follow a completed restart:\n{ops_log}"
    );

    // Negative control: without the persisted writeback the record keeps saying
    // `codex` and the exact reported symptom comes back. This is what makes the
    // assertions above meaningful rather than vacuous.
    let mut stale = SimWorld::new(7_105);
    stale.apply(SimCommand::BindRouteOwner).unwrap();
    stale.apply(SimCommand::SupervisorReady).unwrap();
    stale
        .apply(SimCommand::DisableActorHarnessRecordWriteback)
        .unwrap();
    stale
        .apply(SimCommand::SwitchFrontmatterHarness {
            from: "codex",
            to: "claude",
        })
        .unwrap();
    stale
        .apply(SimCommand::SupervisorHarnessSwitchTick)
        .unwrap();
    stale
        .apply(SimCommand::PerformDeferredHarnessRestart)
        .unwrap();
    stale
        .apply(SimCommand::DispatchRouteAfterHarnessRestart)
        .unwrap();
    assert_eq!(
        stale.coverage.actor_harness_record_writebacks, 0,
        "the regression control must not persist the switch"
    );
    assert_eq!(
        stale.coverage.actor_switch_post_restart_dispatches, 0,
        "the stale record must block the dispatch (the reported bug)"
    );
    let stale_log = stale.ops_log.join("\n");
    assert!(
        stale_log.contains(
            "route_harness_switch_handoff_accepted stored_harness=codex expected_harness=claude-code"
        ),
        "the control must preserve the stale-record handoff marker:\n{stale_log}"
    );
}

#[test]
fn route_sim_harness_switch_accepts_handoff_then_idle_watch_drives_fresh_restart() {
    // `#actorswitchdefer` Part B: the operator switched the doc frontmatter
    // `agent: codex → opencode` while a HEALTHY codex authoritative actor owns the
    // live pane (the sampleportal.md report). Route must DEFER (not replace
    // the live codex pane), and the supervisor idle-watch must drive the deferred
    // restart sequence at a quiet dispatch-ready boundary:
    //   harness_change_detected → agent_restart_triggered → agent_restart_performed.
    let mut world = SimWorld::new(7_101);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    assert_eq!(world.route.durable.lifecycle, SupervisorLifecycle::Ready);

    // The frontmatter `agent:` flips codex→opencode while the codex actor is live.
    world
        .apply(SimCommand::SwitchFrontmatterHarness {
            from: "codex",
            to: "opencode",
        })
        .unwrap();

    // (a) Route accepts a boundary handoff instead of replacing or injecting the
    // live old-harness pane.
    let before_handoffs = world.coverage.actor_switch_route_handoffs_accepted;
    world
        .apply(SimCommand::DispatchRouteAfterHarnessSwitch)
        .unwrap();
    assert_eq!(
        world.coverage.actor_switch_route_handoffs_accepted,
        before_handoffs + 1,
        "a healthy old-harness actor must accept a safe handoff, not be replaced"
    );
    assert_eq!(
        world.coverage.route_dispatch_acceptances, 0,
        "an accepted handoff must NOT accept a prompt dispatch into the old pane"
    );
    assert_eq!(
        world.coverage.actor_switch_restart_disabled_bails, 0,
        "agent_change_restart is ON, so the route must not take the disabled-bail path"
    );
    let ops_log = world.ops_log.join("\n");
    assert!(
        ops_log.contains("route_harness_switch_handoff_accepted")
            && ops_log.contains("action=accepted_boundary_handoff")
            && ops_log.contains("dispatch_old_harness=false")
            && ops_log.contains("auto_trigger=new_harness"),
        "route must log the accepted non-dispatchable boundary handoff:\n{ops_log}"
    );

    // (b) The supervisor idle-watch drives the restart sequence. At a quiet
    // dispatch-ready boundary (actor Ready, queue resumed) the gate returns
    // `Restart`, emitting harness_change_detected → agent_restart_triggered.
    world
        .apply(SimCommand::SupervisorHarnessSwitchTick)
        .unwrap();
    assert_eq!(
        world.coverage.actor_switch_changes_detected, 1,
        "the idle-watch must DETECT the harness change"
    );
    assert_eq!(
        world.coverage.actor_switch_restarts_triggered, 1,
        "a quiet dispatch-ready boundary must TRIGGER the fresh restart"
    );

    // The supervisor restart loop respawns opencode fresh: agent_restart_performed.
    world
        .apply(SimCommand::PerformDeferredHarnessRestart)
        .unwrap();
    assert_eq!(
        world.coverage.actor_switch_restarts_performed, 1,
        "the deferred restart must be PERFORMED, completing the switch"
    );

    // The full ordered sequence must be present, in order.
    let ops_log = world.ops_log.join("\n");
    let detected = ops_log
        .find("harness_change_detected")
        .expect("harness_change_detected must be logged");
    let triggered = ops_log
        .find("agent_restart_triggered")
        .expect("agent_restart_triggered must be logged");
    let performed = ops_log
        .find("agent_restart_performed")
        .expect("agent_restart_performed must be logged");
    assert!(
        detected < triggered && triggered < performed,
        "restart-flow markers must appear in order detected→triggered→performed:\n{ops_log}"
    );
    assert!(
        ops_log.contains("old=codex new=opencode")
            && ops_log.contains("old_harness=codex new_harness=opencode"),
        "the restart-flow markers must name the codex→opencode switch:\n{ops_log}"
    );

    // After the switch the supervisor now runs opencode — a further tick is a no-op
    // (no standing change), proving the switch is fully resolved (not stuck pending).
    let detected_before = world.coverage.actor_switch_changes_detected;
    world
        .apply(SimCommand::SupervisorHarnessSwitchTick)
        .unwrap();
    assert_eq!(
        world.coverage.actor_switch_changes_detected, detected_before,
        "once launch==frontmatter the switch is resolved; no further change is detected"
    );
}

#[test]
fn route_sim_harness_switch_paused_supervisor_holds_for_resume_no_restart() {
    // `#actorswitchdefer` Part B regression: this is the dead-end the operator hit —
    // the codex→opencode switch deferred, but the supervisor was PAUSED (stale
    // #rt83/#qflood pause), so the idle-watch could not reach the restart boundary.
    // The accepted handoff must NOT be a silent drop: route records queue resume as
    // the prerequisite, and the idle-watch detects the change but holds it pending.
    let mut world = SimWorld::new(7_102);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    // The stale supervisor pause: the idle-watch drain boundary cannot fire.
    world.apply(SimCommand::AdminPauseQueue).unwrap();
    assert!(matches!(
        world.route.queue_control,
        QueueControlState::Paused
    ));

    world
        .apply(SimCommand::SwitchFrontmatterHarness {
            from: "codex",
            to: "opencode",
        })
        .unwrap();

    // Route accepts the handoff and names queue resume as the exact prerequisite.
    world
        .apply(SimCommand::DispatchRouteAfterHarnessSwitch)
        .unwrap();
    assert_eq!(
        world.coverage.actor_switch_route_handoffs_accepted, 1,
        "a paused supervisor must retain the accepted live-actor handoff"
    );
    assert_eq!(
        world.coverage.actor_switch_queue_resume_holds, 1,
        "the paused handoff must be held for queue resume"
    );
    let ops_log = world.ops_log.join("\n");
    assert!(
        ops_log.contains("queue_paused=true")
            && ops_log.contains("prerequisite=resume_queue_if_paused")
            && !ops_log.contains("restart-supervisor"),
        "the paused handoff must require resume without prescribing restart:\n{ops_log}"
    );

    // The idle-watch ticks repeatedly while paused. It must DETECT the change every
    // time (so the switch is never silently lost) but NEVER trigger a restart into
    // the paused supervisor — the switch is held pending the boundary.
    for _ in 0..5 {
        world
            .apply(SimCommand::SupervisorHarnessSwitchTick)
            .unwrap();
    }
    assert_eq!(
        world.coverage.actor_switch_changes_detected, 5,
        "the deferred switch must keep being detected (no silent drop) while paused"
    );
    assert_eq!(
        world.coverage.actor_switch_restarts_triggered, 0,
        "no restart may be triggered into a paused supervisor (no boundary reached)"
    );
    assert_eq!(
        world.coverage.actor_switch_restarts_performed, 0,
        "no restart may be performed while the switch is held pending"
    );
    // The pending switch is still present in the model — nothing dropped it.
    assert_eq!(world.route.queue_control, QueueControlState::Paused);

    // Operator resumes the queue: the boundary reopens and the next tick drives the
    // held switch through to a fresh restart — no manual supervisor restart.
    world.apply(SimCommand::AdminResumeQueue).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    world
        .apply(SimCommand::SupervisorHarnessSwitchTick)
        .unwrap();
    assert_eq!(
        world.coverage.actor_switch_restarts_triggered, 1,
        "after resume the held switch must finally trigger the fresh restart"
    );
    world
        .apply(SimCommand::PerformDeferredHarnessRestart)
        .unwrap();
    assert_eq!(
        world.coverage.actor_switch_restarts_performed, 1,
        "the recovered switch must complete with agent_restart_performed"
    );
}

#[test]
fn route_sim_harness_switch_disabled_restart_bails_explicitly_no_silent_proceed() {
    // `#actorswitchdefer` Part B: when `agent_change_restart` is DISABLED the
    // idle-watch will NEVER restart on a harness change, so the route defer would be
    // a permanent dead-end. Route must bail EXPLICITLY with that fact rather than
    // silently proceeding, replacing the pane, or handing back a restart-supervisor
    // hint that will not switch harnesses.
    let mut world = SimWorld::new(7_103);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    world.apply(SimCommand::DisableAgentChangeRestart).unwrap();

    world
        .apply(SimCommand::SwitchFrontmatterHarness {
            from: "codex",
            to: "opencode",
        })
        .unwrap();

    world
        .apply(SimCommand::DispatchRouteAfterHarnessSwitch)
        .unwrap();

    // Explicit disabled-bail — NOT a silent proceed and NOT a pane replacement.
    assert_eq!(
        world.coverage.actor_switch_restart_disabled_bails, 1,
        "a disabled agent_change_restart must take the explicit disabled-bail path"
    );
    assert_eq!(
        world.coverage.actor_switch_route_handoffs_accepted, 0,
        "a disabled automatic switch must not claim an accepted handoff"
    );
    assert_eq!(
        world.coverage.route_dispatch_acceptances, 0,
        "the disabled bail must NOT silently proceed with a dispatch into the old pane"
    );
    assert_eq!(
        world.coverage.actor_switch_queue_resume_holds, 0,
        "the disabled bail must not claim a queue-resume hold"
    );
    let ops_log = world.ops_log.join("\n");
    assert!(
        ops_log.contains("agent_change_restart=disabled")
            && ops_log.contains("action=bail_restart_disabled"),
        "the disabled bail must explicitly state agent_change_restart=disabled:\n{ops_log}"
    );
    assert!(
        !ops_log.contains("action=defer_to_boundary_restart"),
        "a disabled restart must not claim a boundary restart will fire:\n{ops_log}"
    );

    // Even if the idle-watch ticks, the knob-off gate is a no-op: the change is
    // detected (observable) but NO restart is ever triggered/performed.
    for _ in 0..3 {
        world
            .apply(SimCommand::SupervisorHarnessSwitchTick)
            .unwrap();
    }
    assert_eq!(
        world.coverage.actor_switch_restarts_triggered, 0,
        "a knob-off idle-watch must never trigger a restart"
    );
    assert_eq!(
        world.coverage.actor_switch_restarts_performed, 0,
        "a knob-off idle-watch must never perform a restart"
    );
}

#[test]
fn route_sim_dispatch_defers_during_recycle_then_injects_once_after_settle() {
    // `#jbdisprecycle` R4: a JB `Run Agent Doc` dispatch that lands while the
    // project supervisor is mid-`execve` recycle (lib-install auto-recycle /
    // operator restart) must fail closed and inject NOTHING — a trigger typed
    // across the hot-reload boundary has its submit Enter dropped (the live
    // typed-without-submit no-submit repro). Repeated dispatches during the
    // recycle window must NOT stack duplicate un-submitted triggers. Once the
    // fresh supervisor settles and clears the recycle marker, the same dispatch
    // injects the trigger exactly once (R1 defer-until-settle + R3 submit-once).
    let mut world = SimWorld::new(2_007);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();

    // The supervisor enters its lib-install auto-recycle hot-reload window.
    world
        .apply(SimCommand::MarkSupervisorRecycleInflight)
        .unwrap();
    assert!(world.route.recycle_inflight);

    // Every dispatch that lands mid-recycle fails closed — no inject, no re-type.
    for _ in 0..7 {
        world
            .apply(SimCommand::DispatchDuringSupervisorRecycle)
            .unwrap();
    }
    assert_eq!(
        world.coverage.dispatch_into_recycling_pane_blocks, 7,
        "every dispatch during the recycle window must fail closed (no trigger typed across the execve boundary)"
    );
    assert_eq!(
        world.coverage.route_dispatch_acceptances, 0,
        "no trigger may be injected while the supervisor is mid-recycle"
    );
    assert_eq!(
        world.coverage.dispatch_injects, 0,
        "no dispatch_inject marker while mid-recycle (no ~7 stacked un-submitted triggers)"
    );
    let ops_log = world.ops_log.join("\n");
    assert!(
        ops_log.contains("dispatch_into_recycling_pane")
            && ops_log.contains("reason=supervisor_mid_recycle_before_dispatch_send"),
        "ops log must record dispatch_into_recycling_pane for the recycle race:\n{ops_log}"
    );
    assert!(
        !ops_log.contains("dispatch_inject"),
        "no dispatch_inject marker may be logged while the supervisor is mid-recycle:\n{ops_log}"
    );

    // The fresh post-recycle supervisor settles and clears the marker; the same
    // dispatch now injects the trigger exactly once and is proven submitted.
    world.apply(SimCommand::SettleSupervisorRecycle).unwrap();
    assert!(!world.route.recycle_inflight);
    world
        .apply(SimCommand::DispatchDuringSupervisorRecycle)
        .unwrap();
    world.apply(SimCommand::ProveDispatchAccepted).unwrap();

    assert_eq!(
        world.coverage.route_dispatch_acceptances, 1,
        "after the recycle settles the dispatch must submit exactly once"
    );
    assert_eq!(world.coverage.route_dispatch_proofs, 1);
    assert_eq!(
        world.coverage.dispatch_into_recycling_pane_blocks, 7,
        "the settled dispatch must not re-trip the recycle block"
    );
    assert_eq!(
        world.coverage.dispatch_injects, 1,
        "the settled dispatch must inject the trigger exactly once (no stacked copies)"
    );
    let ops_log = world.ops_log.join("\n");
    assert!(
        ops_log.contains("dispatch_inject pane=") && ops_log.contains("attempt=1"),
        "ops log must record the single dispatch_inject attempt=1 after settle:\n{ops_log}"
    );
    assert_eq!(
        ops_log
            .matches("route_dispatch_submit_recycle_settle")
            .count(),
        1,
        "ops log must record exactly one recycle-settle submit marker:\n{ops_log}"
    );
    assert_eq!(
        ops_log.matches("dispatch_start_proof").count(),
        1,
        "ops log must record exactly one dispatch-start proof after settle:\n{ops_log}"
    );
    assert!(
        !ops_log.contains("attempt=2"),
        "a correct recycle-settle dispatch must never log a second dispatch_inject attempt:\n{ops_log}"
    );
    assert!(
        !ops_log.contains("start_session failed"),
        "recycle-boundary proof must not strand a terminal start_session failure:\n{ops_log}"
    );
}

#[test]
fn route_sim_repairs_stale_busy_projection_with_ready_prompt_then_dispatches() {
    // #run-agent-doc-stale-busy-replay / #snrun: a dispatch-only reroute that
    // finds the actor projected Busy but the live pane proving a dispatch-ready
    // prompt must repair the stale projection and DISPATCH, not enqueue into
    // agent:queue auto. Replays the stale-busy projection end-to-end through the
    // SimWorld actor model (the pure predicate is unit-tested separately).
    let mut world = SimWorld::new(2_026);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();

    // Stale busy projection: the actor reports Busy, so a dispatch fails closed
    // (queues) rather than dispatching.
    world.apply(SimCommand::SupervisorBusy).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(
        world.coverage.busy_dispatch_blocks, 1,
        "a busy projection without a proven ready prompt must fail closed (queue)"
    );
    assert_eq!(world.coverage.route_dispatch_acceptances, 0);

    // The live pane proves a dispatch-ready prompt on the current generation:
    // direct idle evidence repairs the stale busy projection to Ready.
    world
        .apply(SimCommand::RepairBusyProjectionWithReadyPrompt)
        .unwrap();
    assert_eq!(world.coverage.busy_projection_ready_repairs, 1);

    // After repair the reroute dispatches to the proven-ready pane instead of
    // enqueuing.
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    world.apply(SimCommand::ProveDispatchAccepted).unwrap();
    assert_eq!(
        world.coverage.route_dispatch_acceptances, 1,
        "after the stale busy projection is repaired the prompt must dispatch, not enqueue"
    );
    assert_eq!(world.coverage.route_dispatch_proofs, 1);
}

#[test]
fn route_sim_stale_busy_repair_requires_busy_lifecycle() {
    // The repair is fail-closed: it only promotes a genuinely Busy projection.
    // A Ready actor is not a stale-busy case, so the repair must be rejected
    // (recorded as a block) and leave dispatch counts untouched.
    let mut world = SimWorld::new(2_027);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    world
        .apply(SimCommand::RepairBusyProjectionWithReadyPrompt)
        .unwrap();
    assert_eq!(
        world.coverage.busy_projection_ready_repairs, 0,
        "repair must not fire when the actor is not projected Busy"
    );
}

#[test]
fn route_sim_dead_supervisor_safe_caller_reaps_stale_socket_and_cold_starts() {
    // `#supdead-coldstart-fallback`: the supervisor process died abruptly, leaving
    // a stale socket (connect → ECONNREFUSED). `restart-supervisor` / `admin recycle`
    // restart a LIVE supervisor in place, but cannot bootstrap a DEAD one. From a
    // SAFE caller (not the dead supervisor's own ancestor) with a reachable tmux
    // target, the production decision reaps the stale socket and cold-starts a fresh
    // supervisor through the route path, reaching Ready.
    let mut world = SimWorld::new(7_301);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    assert_eq!(world.route.durable.lifecycle, SupervisorLifecycle::Ready);
    assert_eq!(world.route.socket, SupervisorSocket::Live);

    // The supervisor PROCESS dies abruptly, leaving a stale socket file behind.
    world
        .apply(SimCommand::AbandonSupervisorToDeadSocket)
        .unwrap();
    assert_eq!(world.route.durable.lifecycle, SupervisorLifecycle::Dead);
    assert_eq!(world.route.socket, SupervisorSocket::StaleRefused);
    assert!(
        world.route.socket.present_but_not_accepting(),
        "a dead supervisor's socket must be present on disk but not accepting"
    );
    assert!(
        world.route.socket.is_dead(),
        "the stale socket must classify as SocketLiveness::Dead for the recovery decision"
    );
    assert_eq!(world.coverage.supervisor_deaths, 1);

    // A safe caller (not the dead supervisor's own ancestor) with a reachable tmux
    // target: the production decision resolves to ColdStart.
    let decision = world.recover_dead_supervisor(false, true);
    assert_eq!(
        decision,
        crate::session_actor_cmd::DeadSupervisorRecovery::ColdStart,
        "a safe caller with a reachable tmux target must cold-start, not surface guidance"
    );
    assert_eq!(world.coverage.dead_supervisor_cold_starts, 1);
    assert_eq!(
        world.coverage.dead_supervisor_guidance_refusals, 0,
        "the safe cold-start path must not surface guidance"
    );

    // The stale socket was reaped and the fresh supervisor bound a new live socket,
    // coming up Starting at a new generation.
    assert_eq!(world.route.socket, SupervisorSocket::Live);
    assert_eq!(world.route.durable.lifecycle, SupervisorLifecycle::Starting);
    let ops_log = world.ops_log.join("\n");
    let reaped = ops_log
        .find("supervisor_cold_start_reaped_stale_socket")
        .expect("the stale socket must be reaped before cold-start");
    let started = ops_log
        .find("supervisor_cold_start decision=ColdStart action=route_auto_start")
        .expect("the fresh supervisor must cold-start through the route path");
    assert!(
        reaped < started,
        "the stale socket must be reaped BEFORE the fresh cold-start binds:\n{ops_log}"
    );

    // The fresh cold-started supervisor reaches a dispatch-ready Ready prompt.
    world.apply(SimCommand::SupervisorReady).unwrap();
    assert_eq!(world.route.durable.lifecycle, SupervisorLifecycle::Ready);
    // A dispatch now lands on the proven-ready fresh pane (recovery is complete).
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(
        world.coverage.route_dispatch_acceptances, 1,
        "after the cold-start recovery the prompt must dispatch to the fresh pane"
    );
}

#[test]
fn route_sim_dead_supervisor_own_ancestor_caller_refuses_with_guidance_no_raw_econnrefused() {
    // `#supdead-coldstart-fallback`: the dead supervisor's recovery caller IS the
    // supervisor's own pane/ancestor — an in-process cold-start would be unsafe
    // (self-targeting). The production decision must refuse with actionable guidance
    // rather than cold-start, and must NOT surface a raw ECONNREFUSED. The supervisor
    // stays Dead (no state change), so the operator can recover from a different pane.
    let mut world = SimWorld::new(7_302);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    world
        .apply(SimCommand::AbandonSupervisorToDeadSocket)
        .unwrap();
    let generation_before = world.route.durable.generation;

    // caller_is_own_ancestor = true → Guidance, even though a tmux target is reachable.
    let decision = world.recover_dead_supervisor(true, true);
    let crate::session_actor_cmd::DeadSupervisorRecovery::Guidance(message) = &decision else {
        panic!("an own-ancestor caller must refuse with Guidance, got {decision:?}");
    };
    assert!(
        message.contains("refusing an unsafe in-process cold-start"),
        "the guidance must name the unsafe in-process cold-start refusal:\n{message}"
    );
    assert!(
        !message.contains("Connection refused") && !message.contains("os error 111"),
        "the guidance must NOT surface a raw ECONNREFUSED:\n{message}"
    );
    assert_eq!(world.coverage.dead_supervisor_guidance_refusals, 1);
    assert_eq!(
        world.coverage.dead_supervisor_cold_starts, 0,
        "an own-ancestor caller must not cold-start"
    );

    // No state change: the supervisor stays Dead with its stale socket lingering.
    assert_eq!(world.route.durable.lifecycle, SupervisorLifecycle::Dead);
    assert_eq!(world.route.socket, SupervisorSocket::StaleRefused);
    assert_eq!(
        world.route.durable.generation, generation_before,
        "a refused cold-start must not advance the generation"
    );
    let ops_log = world.ops_log.join("\n");
    assert!(
        ops_log
            .contains("dead_supervisor_recovery decision=Guidance action=refuse_unsafe_cold_start"),
        "the refusal must be logged as a guidance decision:\n{ops_log}"
    );
    assert!(
        !ops_log.contains("supervisor_cold_start_reaped_stale_socket"),
        "a refused recovery must not reap the stale socket:\n{ops_log}"
    );
}

#[test]
fn route_sim_dead_supervisor_no_tmux_target_refuses_with_actionable_guidance() {
    // `#supdead-coldstart-fallback`: the dead supervisor's recovery caller is safe
    // (not own-ancestor) but no tmux target session is reachable from here, so a
    // route-owned replacement pane cannot be spawned. The production decision must
    // refuse with an actionable message (run `Run Agent Doc` from inside the editor's
    // tmux session) and leave the supervisor Dead — not a raw ECONNREFUSED.
    let mut world = SimWorld::new(7_303);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    world
        .apply(SimCommand::AbandonSupervisorToDeadSocket)
        .unwrap();

    // caller_is_own_ancestor = false, can_resolve_tmux_target = false → Guidance.
    let decision = world.recover_dead_supervisor(false, false);
    let crate::session_actor_cmd::DeadSupervisorRecovery::Guidance(message) = &decision else {
        panic!("an unreachable tmux target must refuse with Guidance, got {decision:?}");
    };
    assert!(
        message.contains("no tmux target session is reachable"),
        "the guidance must explain why the cold-start could not run:\n{message}"
    );
    assert!(
        message.contains("Run Agent Doc") || message.contains("agent-doc start --route-owned"),
        "the guidance must hand back an actionable recovery command:\n{message}"
    );
    assert!(
        !message.contains("Connection refused") && !message.contains("os error 111"),
        "the guidance must NOT surface a raw ECONNREFUSED:\n{message}"
    );
    assert_eq!(world.coverage.dead_supervisor_guidance_refusals, 1);
    assert_eq!(world.coverage.dead_supervisor_cold_starts, 0);

    // The supervisor stays Dead with its stale socket lingering (no cold-start).
    assert_eq!(world.route.durable.lifecycle, SupervisorLifecycle::Dead);
    assert_eq!(world.route.socket, SupervisorSocket::StaleRefused);
    assert!(
        world.route.socket.present_but_not_accepting(),
        "the stale socket must still be present-but-not-accepting after a refused recovery"
    );
}

#[test]
fn control_plane_sim_queue_controls_block_and_resume_route_dispatch() {
    let mut world = SimWorld::new(2_101);
    world.apply(SimCommand::SupervisorReady).unwrap();

    world.apply(SimCommand::AdminPauseQueue).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(world.coverage.queue_pauses, 1);
    assert_eq!(world.coverage.queue_paused_dispatch_blocks, 1);
    assert_eq!(world.coverage.route_dispatch_acceptances, 0);

    world.apply(SimCommand::AdminResumeQueue).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    world.apply(SimCommand::ProveDispatchAccepted).unwrap();
    assert_eq!(world.coverage.queue_resumes, 1);
    assert_eq!(world.coverage.route_dispatch_acceptances, 1);
    assert_eq!(world.coverage.route_dispatch_proofs, 1);

    world.apply(SimCommand::SupervisorBusy).unwrap();
    world.apply(SimCommand::AdminDrainQueue).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(world.coverage.queue_drains, 1);
    assert_eq!(world.coverage.actor_busy_draining_blocks, 1);
    assert_eq!(world.coverage.queue_backpressure_events, 2);
}

#[test]
fn control_plane_sim_admin_handoff_and_reap_require_current_generation() {
    let mut world = SimWorld::new(2_102);
    world.apply(SimCommand::SupervisorReady).unwrap();

    world.apply(SimCommand::AdminHandoffStale).unwrap();
    assert_eq!(world.coverage.stale_generation_blocks, 1);
    assert_eq!(world.coverage.admin_handoffs, 0);

    let prior_generation = world.route.durable.generation;
    world.apply(SimCommand::AdminHandoff).unwrap();
    assert_eq!(world.coverage.admin_handoffs, 1);
    assert_eq!(world.route.durable.generation, prior_generation + 1);
    assert_eq!(world.route.durable.lifecycle, SupervisorLifecycle::Ready);
    assert_eq!(world.route.projection, world.route.durable);

    world.apply(SimCommand::AdminReapStale).unwrap();
    assert_eq!(world.coverage.stale_generation_blocks, 2);
    assert_eq!(world.coverage.admin_reaps, 0);

    world.apply(SimCommand::AdminReap).unwrap();
    assert_eq!(world.coverage.admin_reaps, 1);
    assert_eq!(world.route.durable.lifecycle, SupervisorLifecycle::Closed);
    assert_eq!(world.route.durable.pane_id, None);
}

#[test]
fn control_plane_sim_supervisor_heartbeat_repairs_projection_after_drift() {
    let mut world = SimWorld::new(2_103);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    world.apply(SimCommand::DriftProjection).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(world.coverage.projection_drift_blocks, 1);
    assert_eq!(world.coverage.route_dispatch_acceptances, 0);

    world.apply(SimCommand::SupervisorHeartbeatStale).unwrap();
    assert_eq!(world.coverage.supervisor_heartbeat_stale_blocks, 1);

    world
        .apply(SimCommand::SupervisorHeartbeatReattach)
        .unwrap();
    assert_eq!(world.coverage.supervisor_heartbeat_reattaches, 1);
    assert_eq!(world.route.projection, world.route.durable);

    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    world.apply(SimCommand::ProveDispatchAccepted).unwrap();
    assert_eq!(world.coverage.route_dispatch_acceptances, 1);
    assert_eq!(world.coverage.route_dispatch_proofs, 1);
}

#[test]
fn route_sim_blocks_starting_to_busy_bootstrap_until_current_ready_prompt() {
    let mut world = SimWorld::new(2_006);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(world.coverage.route_dispatch_acceptances, 0);

    world.apply(SimCommand::SupervisorBusy).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(world.coverage.busy_dispatch_blocks, 1);
    assert_eq!(world.coverage.route_dispatch_acceptances, 0);

    world.apply(SimCommand::SupervisorReady).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    world.apply(SimCommand::ProveDispatchAccepted).unwrap();

    assert_eq!(world.coverage.route_dispatch_acceptances, 1);
    assert_eq!(world.coverage.route_dispatch_proofs, 1);
}

#[test]
fn restart_supervisor_sim_refuses_alive_busy_without_force_then_force_kills() {
    let mut world = SimWorld::new(2_016);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorBusy).unwrap();

    let err = world
        .restart_supervisor(false, RestartInterruptOutcome::StillBusy)
        .expect_err("busy restart without --force must fail closed");
    assert!(err.to_string().contains("session_restart refused"));
    assert!(err.to_string().contains("pane %2 is alive-busy"));
    assert!(err.to_string().contains("pass `--force`"));
    assert_eq!(world.coverage.session_restart_busy_refusals, 1);
    assert_eq!(world.coverage.session_restarts, 0);

    world
        .restart_supervisor(true, RestartInterruptOutcome::StillBusy)
        .unwrap();

    assert_eq!(world.coverage.session_restart_force_used, 1);
    assert_eq!(world.coverage.session_restart_busy_force_killed, 1);
    assert_eq!(world.coverage.session_restart_busy_pre_interrupt_idle, 0);
    assert_eq!(world.coverage.session_restarts, 1);
    assert_eq!(world.route.durable.lifecycle, SupervisorLifecycle::Starting);
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(
        world.coverage.starting_dispatch_blocks, 1,
        "route must remain prompt-gated while the restarted supervisor is booting"
    );
}

#[test]
fn restart_supervisor_sim_records_pre_interrupt_idle_before_restart() {
    let mut world = SimWorld::new(2_017);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorBusy).unwrap();

    world
        .restart_supervisor(true, RestartInterruptOutcome::IdleBeforeForceKill)
        .unwrap();

    assert_eq!(world.coverage.session_restart_force_used, 1);
    assert_eq!(world.coverage.session_restart_busy_pre_interrupt_idle, 1);
    assert_eq!(world.coverage.session_restart_busy_force_killed, 0);
    assert_eq!(world.coverage.session_restarts, 1);
    assert_eq!(world.route.durable.lifecycle, SupervisorLifecycle::Starting);
}

#[test]
fn restart_supervisor_sim_force_allows_starting_owner_restart() {
    let mut world = SimWorld::new(2_018);
    world.apply(SimCommand::BindRouteOwner).unwrap();

    let err = world
        .restart_supervisor(false, RestartInterruptOutcome::StillBusy)
        .expect_err("starting restart without --force must fail closed");
    assert!(err.to_string().contains("session_restart refused"));
    assert!(
        err.to_string()
            .contains("the authoritative actor is still starting")
    );
    assert!(
        err.to_string()
            .contains("the document changed after the last committed cycle")
    );
    assert!(err.to_string().contains("Pass `--force`"));
    assert_eq!(world.coverage.session_restart_busy_refusals, 1);
    assert_eq!(world.coverage.session_restarts, 0);

    world
        .restart_supervisor(true, RestartInterruptOutcome::StillBusy)
        .unwrap();

    assert_eq!(world.coverage.session_restarts, 1);
    assert_eq!(world.route.durable.lifecycle, SupervisorLifecycle::Starting);
}

#[test]
fn route_sim_coalesces_repeated_starting_timeouts_for_same_generation() {
    let mut world = SimWorld::new(2_009);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();

    assert_eq!(world.coverage.starting_timeout_records, 1);
    assert_eq!(world.coverage.starting_timeout_coalesces, 1);
    assert_eq!(world.coverage.route_dispatch_acceptances, 0);

    world.apply(SimCommand::PromoteStartingPromptReady).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    world.apply(SimCommand::ProveDispatchAccepted).unwrap();

    assert_eq!(world.coverage.route_dispatch_acceptances, 1);
    assert_eq!(world.coverage.route_dispatch_proofs, 1);
}

#[test]
fn qflood_coalesces_in_flight_auto_redispatch_and_releases_after_proof() {
    // #qflood: while a cycle's first dispatch is in flight (accepted, unproven), an
    // AUTO re-dispatch (route auto-start on a file-change save, idle continuation,
    // `/loop` tick) must be COALESCED — not piled into the busy pane — and the
    // queue must keep running (no pause). This is the deterministic repro of the
    // operator's "triggers accumulate in the harness composer mid-turn" flood.
    let mut world = SimWorld::new(2_205);
    world.apply(SimCommand::SupervisorReady).unwrap();

    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(world.coverage.route_dispatch_acceptances, 1);

    // Re-fire twice while the first dispatch is still in flight ⇒ both coalesced.
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(
        world.coverage.route_dispatch_acceptances, 1,
        "redundant in-flight re-dispatches must not pile up in the pane"
    );
    assert_eq!(world.coverage.route_dispatch_coalesced, 2);
    assert_eq!(
        world.coverage.queue_pauses, 0,
        "coalescing is backpressure, never a queue stop"
    );

    // Once the in-flight dispatch is proven (consumed), coalescing RELEASES: the
    // next dispatch is admitted normally — no stall.
    world.apply(SimCommand::ProveDispatchAccepted).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(
        world.coverage.route_dispatch_acceptances, 2,
        "a dispatch after the prior is consumed must be admitted, not coalesced"
    );
    assert_eq!(world.coverage.route_dispatch_coalesced, 2);
}

#[test]
fn controller_recovery_upserts_open_dispatch_marker_instead_of_flooding() {
    let mut world = SimWorld::new(2_208);
    world.apply(SimCommand::SupervisorReady).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(world.coverage.route_dispatch_acceptances, 1);

    world
        .apply(SimCommand::RecoverControllerDispatchMarkers)
        .unwrap();
    assert_eq!(world.route.recovery_marker_keys.len(), 1);

    world
        .apply(SimCommand::RecoverControllerDispatchMarkers)
        .unwrap();
    assert_eq!(
        world.route.recovery_marker_keys.len(),
        1,
        "replaying controller restart recovery for the same open receipt must update one marker key, not append another"
    );
    assert_eq!(world.coverage.recovery_marker_upsert_dedupes, 1);
}

#[test]
fn qflood_operator_dispatch_is_not_coalesced_in_flight() {
    // #qflood: an explicit operator dispatch (JB `Run Agent Doc`) must pass even
    // while an auto dispatch for the same cycle is in flight — operator intent is
    // never blocked by auto-drain backpressure ("type while the queue continues").
    let mut world = SimWorld::new(2_206);
    world.apply(SimCommand::SupervisorReady).unwrap();

    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(world.coverage.route_dispatch_acceptances, 1);

    // An AUTO re-fire here would coalesce; the operator dispatch must NOT.
    world.apply(SimCommand::DispatchOperatorPrompt).unwrap();
    assert_eq!(
        world.coverage.route_dispatch_acceptances, 2,
        "operator dispatch must pass through in-flight backpressure"
    );
    assert_eq!(world.coverage.route_dispatch_coalesced, 0);
}

#[test]
fn qflood2_in_flight_coalesce_is_deduped_success_not_a_dispatch_failure() {
    // #qflood2: the #qflood in-flight coalesce SUPPRESSES the redundant re-send, but
    // it must report DEDUPED-SUCCESS — not a dispatch failure. This is the
    // deterministic analogue of the live route path translating the controller's
    // `coalesced_in_flight` bail into Ok(deduped pane) instead of an exit-1, while a
    // genuinely blocked dispatch (paused queue) still fails closed (the live
    // `dispatch_error_is_coalesced` classifier must not swallow a real `queue_paused`).
    let mut world = SimWorld::new(2_207);
    world.apply(SimCommand::SupervisorReady).unwrap();

    // First dispatch accepted + in flight.
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(world.coverage.route_dispatch_acceptances, 1);

    // Redundant in-flight AUTO re-dispatch ⇒ coalesced. It returns Ok (deduped
    // success): no second trigger piled into the pane AND no failure/backpressure
    // recorded — a coalesce is success, not a suppressed error.
    let coalesced = world.apply(SimCommand::DispatchRoutePrompt);
    assert!(
        coalesced.is_ok(),
        "a benign in-flight coalesce must report deduped-success, not error: {coalesced:?}"
    );
    assert_eq!(world.coverage.route_dispatch_coalesced, 1);
    assert_eq!(
        world.coverage.route_dispatch_acceptances, 1,
        "coalesce must not pile a second trigger into the pane"
    );
    assert_eq!(
        world.coverage.queue_paused_dispatch_blocks, 0,
        "a coalesce is deduped-success, not a queue block"
    );
    assert_eq!(
        world.coverage.queue_backpressure_events, 0,
        "a coalesce must not record a backpressure failure event"
    );

    // Contrast: a genuinely paused queue is NOT a benign coalesce — it must fail
    // closed and record a block, not be reported as deduped-success.
    world.apply(SimCommand::AdminPauseQueue).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(
        world.coverage.queue_paused_dispatch_blocks, 1,
        "a real queue_paused dispatch must record a block, not deduped-success"
    );
    assert_eq!(
        world.coverage.route_dispatch_coalesced, 1,
        "the pause is a block, not a coalesce"
    );
}

#[test]
fn recycle_clear_pipeline_resumes_go_mode_drain_as_a_step() {
    // `#clearcontresume`: the full operator recycle + clear pipeline, driven by the
    // SAME production decision predicates the live supervisor idle-queue watch uses
    // (`supervisor_recycle_action`, `clear_cooldown_resume_ready`,
    // `idle_queue_drain_decision`). The recycle + clear is a continuation *step*: a
    // go-mode drain resumes on its own without an operator route.
    let mut world = SimWorld::new(2_026);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();

    let gen_before = world.route.durable.generation;
    let pane_before = world.route.durable.pane_id.clone();

    // `admin recycle --all-projects` marks recycle at the next idle boundary; a later
    // `cargo install` made the binary stale with auto-recycle opted in.
    world.apply(SimCommand::MarkSupervisorBinaryStale).unwrap();
    world
        .apply(SimCommand::EnableSupervisorAutoRecycle)
        .unwrap();
    world.apply(SimCommand::OperatorRecycleMark).unwrap();

    // First idle tick: the idle boundary recycles in place onto the fresh binary,
    // PRESERVING the live pane (execve) and advancing the generation. No head is
    // waiting yet, so the recycle does not dispatch.
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(world.coverage.supervisor_recycles, 1);
    assert_eq!(
        world.route.durable.generation,
        gen_before + 1,
        "recycle advances the generation"
    );
    assert_eq!(
        world.route.durable.pane_id, pane_before,
        "recycle preserves the live pane via execve (not a cold rebind)"
    );
    assert!(
        !world.recycle_clear.binary_stale,
        "recycle promoted the freshly-installed binary"
    );
    assert_eq!(
        world.coverage.go_drain_dispatches, 0,
        "no head was waiting at the recycle boundary"
    );

    // A go-mode head is now waiting; the operator `session clear` writes the manual
    // clear cooldown before any idle poll observes the head.
    world.apply(SimCommand::ActivateGoModeQueueHead).unwrap();
    world.apply(SimCommand::SessionClear).unwrap();
    assert!(world.recycle_clear.clear_cooldown_active);

    // The cleared pane must settle for CLEAR_COOLDOWN_RESUME_IDLE_TICKS consecutive
    // idle polls before the cooldown auto-expires — earlier ticks must NOT resume.
    for tick in 1..4 {
        world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
        assert!(
            world.recycle_clear.clear_cooldown_active,
            "cooldown must hold at settle tick {tick}"
        );
        assert_eq!(world.coverage.clear_cooldown_resumes, 0);
        assert_eq!(world.coverage.go_drain_dispatches, 0);
    }

    // 4th settled idle tick: the cooldown auto-expires (resume re-evaluates next tick,
    // so no dispatch yet).
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert!(
        !world.recycle_clear.clear_cooldown_active,
        "cooldown auto-expired after the settle debounce"
    );
    assert_eq!(world.coverage.clear_cooldown_resumes, 1);
    assert_eq!(
        world.coverage.go_drain_dispatches, 0,
        "the resume tick re-evaluates; it does not dispatch into the just-cleared pane"
    );

    // Next tick: the normal go-mode drain dispatches the waiting head — the recycle +
    // clear resumed the drain as a step, with no operator route.
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(
        world.coverage.go_drain_dispatches, 1,
        "the go-mode drain resumes after the recycle + clear settles"
    );

    // A stuck head is not re-fired every tick (drain dedup).
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(world.coverage.go_drain_dispatches, 1);
}

#[test]
fn open_agent_doc_cycle_defers_self_recycle_until_finalize_commits() {
    // `#midturn-recycle-resume` regression: a stale-binary self-recycle (the
    // #supautoinstall hot-reload) must NOT `execve` while an agent-doc cycle is open
    // (preflight taken, finalize not yet committed) or an IPC ack connection is in
    // flight. Firing the `execve` mid-cycle tears down the in-flight IPC listener and
    // severs the ack-content round-trip, so the next finalize validates its candidate
    // against the pre-recycle preflight baseline → `live_prompt_drift_after_preflight`
    // → the visible-repair-required wedge + "no response exists to replay"
    // refusal chain the operator hit live. The deferral preserves the live cycle; the
    // recycle fires at the TRUE quiescent boundary once finalize commits.
    let mut world = SimWorld::new(7_777);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();

    let gen_before = world.route.durable.generation;

    // Stale binary + auto-recycle ON — the exact #supautoinstall self-recycle setup —
    // but an agent-doc cycle is OPEN (the finalize has not committed yet).
    world.apply(SimCommand::MarkSupervisorBinaryStale).unwrap();
    world
        .apply(SimCommand::EnableSupervisorAutoRecycle)
        .unwrap();
    world.apply(SimCommand::SetAgentDocCycleOpen(true)).unwrap();

    // Idle ticks at a harness turn boundary while the cycle is open must DEFER the
    // recycle: no execve, generation unchanged, binary stays stale (pending).
    for tick in 1..=3 {
        world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
        assert_eq!(
            world.coverage.supervisor_recycles, 0,
            "an open agent-doc cycle must defer the execve recycle (tick {tick})"
        );
        assert_eq!(
            world.route.durable.generation, gen_before,
            "a deferred recycle must not advance the generation (tick {tick})"
        );
        assert!(
            world.recycle_clear.binary_stale,
            "the stale binary stays pending across the deferral (tick {tick})"
        );
    }

    // The finalize commits — the cycle closes and IPC drains. The next idle tick now
    // hot-reloads onto the fresh binary in place (execve), advancing the generation:
    // the deferred recycle fired at the true quiescent boundary, never mid-finalize.
    world
        .apply(SimCommand::SetAgentDocCycleOpen(false))
        .unwrap();
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(
        world.coverage.supervisor_recycles, 1,
        "once the cycle commits and IPC drains, the deferred recycle fires"
    );
    assert_eq!(
        world.route.durable.generation,
        gen_before + 1,
        "the recycle advances the generation only after the cycle closed"
    );
    assert!(
        !world.recycle_clear.binary_stale,
        "the recycle promoted the freshly-installed binary"
    );
}

#[test]
fn operator_recycle_mark_defers_while_agent_doc_cycle_open() {
    // `#midturn-recycle-resume`: an explicit operator/admin recycle is still a
    // recycle arm. While a preflight->finalize cycle is open, it must remain pending
    // instead of bypassing the policy and execve-ing through the live checkpoint.
    let mut world = SimWorld::new(8_202);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();

    let gen_before = world.route.durable.generation;

    world.apply(SimCommand::MarkSupervisorBinaryStale).unwrap();
    world.apply(SimCommand::OperatorRecycleMark).unwrap();
    world.apply(SimCommand::SetAgentDocCycleOpen(true)).unwrap();

    for tick in 1..=3 {
        world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
        assert_eq!(
            world.coverage.supervisor_recycles, 0,
            "operator recycle must defer while the agent-doc cycle is open (tick {tick})"
        );
        assert!(
            world.recycle_clear.operator_recycle_marked,
            "the operator recycle mark stays pending while deferred"
        );
    }

    world
        .apply(SimCommand::SetAgentDocCycleOpen(false))
        .unwrap();
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();

    assert_eq!(
        world.coverage.supervisor_recycles, 1,
        "the pending operator recycle fires after the cycle commits"
    );
    assert!(
        !world.recycle_clear.operator_recycle_marked,
        "the operator recycle mark is consumed only by an actual recycle"
    );
    assert_eq!(world.route.durable.generation, gen_before + 1);
}

#[test]
fn never_closing_cycle_escalates_recycle_then_boot_redispatches_interrupted_turn() {
    // `#midturn-recycle-resume` Phase B: Phase A defers a stale-binary self-recycle
    // while a cycle is open, but a cycle that NEVER closes (a wedged finalize, a
    // stranded preflight) must not starve the recycle forever. Past
    // `MAX_CYCLE_OPEN_DEFER_TICKS` consecutive boundary deferrals the watch ESCALATES
    // and forces the recycle. The forced `execve` severs the wedged cycle, so the
    // harness child dies — and the fresh supervisor boot re-dispatches the
    // genuinely-interrupted turn from the still-open `#durablerecycle` checkpoint
    // (idempotently: a second boot does not re-dispatch again).
    use agent_doc_supervisor::lifecycle::MAX_CYCLE_OPEN_DEFER_TICKS;

    let mut world = SimWorld::new(9_191);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();

    // Stale binary + auto-recycle ON, but a cycle that never closes.
    world.apply(SimCommand::MarkSupervisorBinaryStale).unwrap();
    world
        .apply(SimCommand::EnableSupervisorAutoRecycle)
        .unwrap();
    world.apply(SimCommand::SetAgentDocCycleOpen(true)).unwrap();

    // Tick just under the threshold: every tick DEFERS, no recycle, no escalation.
    for _ in 0..(MAX_CYCLE_OPEN_DEFER_TICKS - 1) {
        world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    }
    assert_eq!(
        world.coverage.supervisor_recycles, 0,
        "an open cycle defers the recycle below the escalation threshold"
    );
    assert_eq!(
        world.coverage.cycle_open_defer_escalations, 0,
        "no escalation before the threshold"
    );
    assert!(
        world.recycle_clear.binary_stale,
        "the stale binary stays pending while the cycle is open"
    );

    // The threshold tick ESCALATES and forces the recycle even though the cycle is
    // STILL open — proving a never-closing cycle cannot starve the recycle.
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(
        world.coverage.cycle_open_defer_escalations, 1,
        "the threshold tick escalates the deferred recycle"
    );
    assert_eq!(
        world.coverage.supervisor_recycles, 1,
        "the escalation forces the recycle over the never-closing cycle"
    );
    assert!(
        !world.recycle_clear.binary_stale,
        "the forced recycle promoted the fresh binary"
    );
    assert!(
        world.recycle_clear.cycle_open,
        "the cycle was still open when the recycle was forced (it never closed)"
    );
    assert!(
        world.recycle_clear.recycle_child_died,
        "the forced execve over a wedged cycle severs/kills the harness child"
    );

    // The fresh supervisor boots: the child died across the recycle, so it
    // re-dispatches the interrupted turn from the still-open checkpoint.
    world.apply(SimCommand::SupervisorRecycleBoot).unwrap();
    assert_eq!(
        world.coverage.recycle_resume_redispatches, 1,
        "the boot re-dispatches the genuinely-interrupted turn"
    );
    assert_eq!(
        world.coverage.recycle_resume_adopt_surviving, 0,
        "no surviving child to adopt — the child died across the recycle"
    );

    // IDEMPOTENCY: a second boot over the same still-open + consumed checkpoint must
    // NOT re-dispatch the turn again.
    world.apply(SimCommand::SupervisorRecycleBoot).unwrap();
    assert_eq!(
        world.coverage.recycle_resume_redispatches, 1,
        "a second boot must not re-dispatch the already-consumed turn"
    );
}

#[test]
fn recycle_boot_with_surviving_child_adopts_without_redispatch() {
    // `#midturn-recycle-resume` Phase B idempotency: the common Phase-A steady state —
    // the harness child SURVIVED the `execve` recycle and is still running the
    // interrupted turn. The fresh boot must ADOPT it without re-dispatching (a
    // re-dispatch would double-run the turn).
    let mut world = SimWorld::new(5_005);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();

    // Cycle open, child survived by default.
    world.apply(SimCommand::SetAgentDocCycleOpen(true)).unwrap();
    world.apply(SimCommand::SupervisorRecycleBoot).unwrap();

    assert_eq!(
        world.coverage.recycle_resume_adopt_surviving, 1,
        "a surviving child is adopted on boot"
    );
    assert_eq!(
        world.coverage.recycle_resume_redispatches, 0,
        "a surviving child must NOT trigger a re-dispatch (no double-run)"
    );
}

#[test]
fn write_wedged_recycles_immediately_even_while_cycle_open() {
    // `#midturn-wedge-recycle`: a proven editor-IPC wedge means the open agent-doc
    // cycle can NEVER close — closeout is blocked on a convergence receipt that will
    // not arrive — so deferring the recycle until the cycle commits would deadlock.
    // The wedge therefore recycles IMMEDIATELY, mid-cycle, even with auto-recycle
    // opted OUT. This is the exact deadlock that previously forced a manual
    // `admin recycle`.
    let mut world = SimWorld::new(8_203);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    world
        .apply(SimCommand::DisableSupervisorAutoRecycle)
        .unwrap();

    world.apply(SimCommand::MarkSupervisorBinaryStale).unwrap();
    world.apply(SimCommand::MarkWriteWedged).unwrap();
    world.apply(SimCommand::SetAgentDocCycleOpen(true)).unwrap();

    // A single tick with the cycle STILL open recycles now — no deferral.
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(
        world.coverage.wedge_triggered_recycles, 1,
        "a proven wedge recycles immediately even while the agent-doc cycle is open"
    );
    assert_eq!(world.coverage.supervisor_recycles, 1);
    assert!(
        !world.recycle_clear.write_wedged,
        "the successful recycle clears the wedge latch (once-per-episode guard)"
    );
}

#[test]
fn write_wedged_recycles_mid_turn_off_boundary() {
    // `#midturn-wedge-recycle`: the whole point — a wedge recycles even when the
    // harness turn is ACTIVE (no turn boundary is reachable). The supervisor is fresh
    // (NOT stale) and auto-recycle is on by default, mirroring this session: the
    // running supervisor matched the installed binary yet the editor-IPC write never
    // converged, so nothing recycled until a manual `admin recycle`.
    let mut world = SimWorld::new(8_207);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();

    // Turn is in flight (Busy) → `turn_boundary` is false. No stale binary.
    world.apply(SimCommand::SupervisorBusy).unwrap();
    world.apply(SimCommand::MarkWriteWedged).unwrap();
    world.apply(SimCommand::SetAgentDocCycleOpen(true)).unwrap();

    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(
        world.coverage.wedge_triggered_recycles, 1,
        "a proven wedge recycles mid-turn (off boundary) on a fresh binary"
    );
    assert_eq!(world.coverage.supervisor_recycles, 1);
    assert!(
        !world.recycle_clear.write_wedged,
        "the mid-turn wedge recycle clears the wedge latch"
    );
}

#[test]
fn write_wedged_recycle_waits_for_first_safe_intra_turn_checkpoint() {
    // `#midturn-wedge-recycle`: the wedge recycle must not `execve` while a supervisor
    // IPC connection is being handled (that would sever the in-flight apply). It waits
    // for the FIRST tick that is a safe intra-turn checkpoint — no in-flight handler —
    // then recycles, still mid-turn (no wait for the full turn boundary).
    let mut world = SimWorld::new(8_211);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();

    world.apply(SimCommand::MarkWriteWedged).unwrap();
    world.apply(SimCommand::SetAgentDocCycleOpen(true)).unwrap();
    // An IPC apply is in flight → this tick is NOT a safe checkpoint.
    world.apply(SimCommand::MarkIpcInflight(true)).unwrap();

    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(
        world.coverage.supervisor_recycles, 0,
        "the wedge recycle defers while an IPC apply is in flight (unsafe checkpoint)"
    );
    assert!(
        world.recycle_clear.write_wedged,
        "the wedge latch stays pending until a safe checkpoint is reached"
    );

    // The in-flight apply drains → the next tick is the first safe checkpoint.
    world.apply(SimCommand::MarkIpcInflight(false)).unwrap();
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(
        world.coverage.wedge_triggered_recycles, 1,
        "the wedge recycles at the first safe intra-turn checkpoint"
    );
    assert_eq!(world.coverage.supervisor_recycles, 1);
    assert!(
        !world.recycle_clear.write_wedged,
        "the safe-checkpoint recycle clears the wedge latch"
    );
}

#[test]
fn restart_supervisor_drains_then_reexecs_in_place_no_dropped_turn() {
    // `#supkill-bg` (#anw0 parts 1+2): an explicit `restart-supervisor` on a stale
    // supervisor is a blue/green DRAIN-and-supersede, driven by the production
    // `supervisor_restart_action` policy. While a turn is in flight the restart DRAINS
    // (no teardown mid-turn); only at the turn boundary does it hot-reload in place via
    // `execve`, preserving the live pane and advancing the generation with NO dropped
    // turn. This is the default healthy restart that fixes the stale-supervisor
    // `generation closed` / `#fcc0` case the kill-first path could not.
    let mut world = SimWorld::new(4_242);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();

    let gen_before = world.route.durable.generation;
    let pane_before = world.route.durable.pane_id.clone();

    // The supervisor's launch binary is stale (a later `cargo install`). The operator
    // runs `restart-supervisor` WHILE a turn is in flight (the pane is Busy).
    world.apply(SimCommand::MarkSupervisorBinaryStale).unwrap();
    world.apply(SimCommand::SupervisorBusy).unwrap();
    world.apply(SimCommand::MarkIpcInflight(true)).unwrap();
    world.apply(SimCommand::RequestSupervisorRestart).unwrap();

    // While supervisor IPC is active, idle ticks must DRAIN, not tear down:
    // `supervisor_restart_action` returns `AwaitDrain`, so the restart stays pending
    // and the live turn (generation/pane) is untouched.
    for tick in 1..=2 {
        world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
        assert_eq!(
            world.coverage.supervisor_restart_drain_reexecs, 0,
            "restart must DRAIN mid-turn (tick {tick}), never reexec into a live turn"
        );
        assert!(
            world.recycle_clear.restart_requested,
            "the restart stays pending while the turn drains (tick {tick})"
        );
        assert_eq!(world.route.durable.generation, gen_before);
        assert!(
            world.recycle_clear.binary_stale,
            "binary still stale mid-turn"
        );
    }

    // The in-flight IPC and turn finish (drain) → the pane returns to a
    // dispatch-ready prompt. The NEXT idle tick hot-reloads in place via `execve`,
    // preserving the pane and advancing the generation.
    world.apply(SimCommand::MarkIpcInflight(false)).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(
        world.coverage.supervisor_restart_drain_reexecs, 1,
        "at the drained boundary the stale restart re-execs in place (default healthy restart)"
    );
    assert!(
        !world.recycle_clear.restart_requested,
        "the restart was served at the boundary"
    );
    assert_eq!(
        world.route.durable.generation,
        gen_before + 1,
        "the in-place reexec advances the generation (supersede)"
    );
    assert_eq!(
        world.route.durable.pane_id, pane_before,
        "the reexec preserves the live pane via execve (no dropped turn, no cold rebind)"
    );
    assert!(
        !world.recycle_clear.binary_stale,
        "the reexec promoted the freshly-installed binary"
    );

    // Idempotent: with no pending restart, later idle ticks do not re-fire.
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(world.coverage.supervisor_restart_drain_reexecs, 1);
}

#[test]
fn stale_restart_reexecs_at_no_ipc_checkpoint_despite_stale_busy_marker() {
    // A prior binary can retain a stale Busy/open-cycle marker after the harness
    // has returned to an idle prompt. In-place execve preserves the harness child,
    // pane, and checkpoint, so the absence of supervisor IPC is the authoritative
    // safe checkpoint; waiting for that stale marker would wedge replacement.
    let mut world = SimWorld::new(4_243);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();

    let generation_before = world.route.durable.generation;
    let pane_before = world.route.durable.pane_id.clone();

    world.apply(SimCommand::MarkSupervisorBinaryStale).unwrap();
    world.apply(SimCommand::SupervisorBusy).unwrap();
    world.apply(SimCommand::SetAgentDocCycleOpen(true)).unwrap();
    world.apply(SimCommand::MarkIpcInflight(false)).unwrap();
    world.apply(SimCommand::RequestSupervisorRestart).unwrap();
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();

    assert_eq!(world.coverage.supervisor_restart_drain_reexecs, 1);
    assert!(!world.recycle_clear.restart_requested);
    assert!(!world.recycle_clear.binary_stale);
    assert_eq!(world.route.durable.generation, generation_before + 1);
    assert_eq!(world.route.durable.pane_id, pane_before);
    assert!(
        world.recycle_clear.cycle_open,
        "in-place replacement preserves the durable open-cycle checkpoint"
    );
}

#[test]
fn accepted_stale_supervisor_replacement_timeout_preserves_mid_turn_session() {
    use agent_doc_controller::supervisor_replacement::{
        SupervisorReplacementEscalation, SupervisorReplacementEscalationFacts,
        SupervisorReplacementIpcOutcome, decide_supervisor_replacement_escalation,
    };

    // Reproduce the live failure: the controller handed a replacement request to
    // a stale supervisor while Codex was mid-turn. The supervisor correctly owned
    // a drain-to-boundary reexec, but the controller's short proof wait expired and
    // escalated to kill + cold-start, interrupting the conversation.
    let mut world = SimWorld::new(4_244);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    world.apply(SimCommand::MarkSupervisorBinaryStale).unwrap();
    world.apply(SimCommand::SupervisorBusy).unwrap();
    world.apply(SimCommand::RequestSupervisorRestart).unwrap();

    let generation_before = world.route.durable.generation;
    let pane_before = world.route.durable.pane_id.clone();
    let decision = decide_supervisor_replacement_escalation(SupervisorReplacementEscalationFacts {
        ipc_outcome: SupervisorReplacementIpcOutcome::Accepted,
        force: false,
        initial_host_stale: true,
    });
    assert_eq!(
        decision,
        SupervisorReplacementEscalation::AwaitAcceptedInPlace,
        "an observation timeout cannot revoke an accepted drain owned by the live supervisor"
    );
    assert_eq!(world.route.durable.generation, generation_before);
    assert_eq!(world.route.durable.pane_id, pane_before);
    assert_eq!(
        world.coverage.supervisor_restart_drain_reexecs, 0,
        "the controller observation leaves the mid-turn child untouched"
    );

    // The existing supervisor eventually reaches the real boundary and performs
    // the already-covered in-place reexec on the same pane.
    world.apply(SimCommand::SupervisorReady).unwrap();
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(world.coverage.supervisor_restart_drain_reexecs, 1);
    assert_eq!(world.route.durable.generation, generation_before + 1);
    assert_eq!(world.route.durable.pane_id, pane_before);
}

#[test]
fn codex_child_termination_restarts_exact_orchard_conversation() {
    use agent_doc_supervisor::session_lineage::HarnessSessionLineage;

    // Reproduce the missing-frontmatter failure with a neutral document name.
    // The Codex prompt hook has published the real thread id to controller state,
    // while the compatibility `resume:` projection is still absent.
    let frontmatter_resume: Option<&str> = None;
    let mut lineage = HarnessSessionLineage::new(None, Some("older-thread".into()));
    assert!(lineage.observe_projected_id(Some("orchard-thread")));
    assert_eq!(frontmatter_resume, None);

    // Timer/reconcile observations and a lagging frontmatter projection cannot
    // replace or erase the controller-owned binding.
    assert!(!lineage.observe_projected_id(Some("older-thread")));
    assert_eq!(lineage.active_id(), Some("orchard-thread"));

    // Child termination is a lifecycle edge, not a request for a new
    // conversation. The replacement argv names the exact id and never uses
    // Codex's process-global `--last` selector.
    let fresh_args = vec![
        "-s".to_string(),
        "danger-full-access".to_string(),
        "--model".to_string(),
        "gpt-5".to_string(),
    ];
    let args = agent_doc_harness::HarnessConfig::codex()
        .exact_resume_args(
            &fresh_args,
            lineage.active_id().expect("controller lineage"),
        )
        .unwrap()
        .unwrap();
    assert_eq!(&args[..2], ["resume", "orchard-thread"]);
    assert!(!args.iter().any(|arg| arg == "--last"));
}

#[test]
fn claude_child_termination_replaces_fresh_session_assignment_with_exact_resume() {
    use agent_doc_supervisor::session_lineage::HarnessSessionLineage;

    // A fresh Claude child is assigned a deterministic conversation id. When
    // that child later terminates, the supervisor reuses its fresh-launch
    // policy args but must replace the assignment with the document's exact
    // lineage instead of emitting the contradictory
    // `--session-id <fresh> --resume <existing>` pair.
    let mut lineage = HarnessSessionLineage::new(
        Some("orchard-conversation".into()),
        Some("older-conversation".into()),
    );
    assert_eq!(lineage.active_id(), Some("orchard-conversation"));
    assert!(!lineage.observe_projected_id(Some("older-conversation")));

    let fresh_args = vec![
        "--dangerously-skip-permissions".to_string(),
        "--model".to_string(),
        "opus".to_string(),
        "--session-id".to_string(),
        "fresh-conversation".to_string(),
    ];
    let args = agent_doc_harness::HarnessConfig::claude()
        .exact_resume_args(
            &fresh_args,
            lineage.active_id().expect("controller lineage"),
        )
        .unwrap()
        .unwrap();

    assert!(
        args.windows(2)
            .any(|window| window == ["--resume", "orchard-conversation"])
    );
    assert!(!args.iter().any(|arg| arg == "--session-id"));
    assert!(!args.iter().any(|arg| arg == "--continue"));
}

#[test]
fn pure_codex_thread_does_not_inherit_orchard_agent_doc_work() {
    use agent_doc_codex_hook_io::SessionState;
    use agent_doc_queue::queue_continuation::QueueContinuation;

    // Thread A explicitly owns an agent-doc document and the document has a
    // durable continuation marker. Thread B starts as a plain Codex session in
    // the same project. The ambient Stop-hook boundary must remain keyed only
    // by B's exact thread id; project-local liveness is not ownership.
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir(project.path().join(".git")).unwrap();
    let document = project.path().join("orchard-notes.md");
    std::fs::write(&document, "# Orchard notes\n").unwrap();
    agent_doc_codex_hook_io::save_state(
        project.path(),
        &SessionState {
            session_id: "orchard-agent-thread".into(),
            doc_path: document.display().to_string(),
            last_turn_id: "turn-a".into(),
            last_prompt: format!("agent-doc {}", document.display()),
            last_auto_queue_head: None,
            last_context_clear_at: None,
            updated_at: 1,
        },
    )
    .unwrap();
    agent_doc_queue_io::continuation_marker::write_continuation_marker(
        &document,
        &QueueContinuation {
            head_prompt: "review orchard offer".into(),
            head_id: Some("orchard-review".into()),
            reason: "ready".into(),
        },
        "simworld",
    )
    .unwrap();

    assert!(
        agent_doc_codex_stop_io::load_bound_session_for_stop(
            project.path(),
            "orchard-agent-thread",
        )
        .unwrap()
        .is_some()
    );
    assert!(
        agent_doc_codex_stop_io::load_bound_session_for_stop(project.path(), "plain-codex-thread",)
            .unwrap()
            .is_none(),
        "the document marker and thread A binding must not attach plain thread B"
    );
}

#[test]
fn restart_supervisor_open_cycle_never_overrides_active_ipc_then_reexecs_when_drained() {
    // A stale open closeout cycle may hit the bounded cycle-open escalation,
    // but that timer must never override an active supervisor IPC handler. Once
    // IPC drains, in-place replacement is immediately safe even though the open
    // cycle remains; execve preserves the child, pane, and durable checkpoint.
    use agent_doc_supervisor::lifecycle::MAX_CYCLE_OPEN_DEFER_TICKS;

    let mut world = SimWorld::new(4_243);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    let gen_before = world.route.durable.generation;
    let pane_before = world.route.durable.pane_id.clone();

    world.apply(SimCommand::MarkSupervisorBinaryStale).unwrap();
    world.apply(SimCommand::SetAgentDocCycleOpen(true)).unwrap();
    world.apply(SimCommand::MarkIpcInflight(true)).unwrap();
    world.apply(SimCommand::RequestSupervisorRestart).unwrap();

    for _ in 0..MAX_CYCLE_OPEN_DEFER_TICKS {
        world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    }
    assert_eq!(
        world.coverage.supervisor_restart_drain_reexecs, 0,
        "active IPC must defer replacement even after the cycle-open escalation"
    );
    assert!(
        world.recycle_clear.restart_requested,
        "the restart request stays pending until supervisor IPC drains"
    );
    assert_eq!(
        world.coverage.cycle_open_defer_escalations, 1,
        "the wedged open cycle still records its bounded escalation"
    );

    world.apply(SimCommand::MarkIpcInflight(false)).unwrap();
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(
        world.coverage.supervisor_restart_drain_reexecs, 1,
        "the pending replacement reexecs at the first no-IPC safe checkpoint"
    );
    assert!(
        !world.recycle_clear.restart_requested,
        "the restart request is consumed by the replacement"
    );
    assert!(
        !world.recycle_clear.binary_stale,
        "the replacement promoted the fresh binary"
    );
    assert_eq!(world.route.durable.generation, gen_before + 1);
    assert_eq!(
        world.route.durable.pane_id, pane_before,
        "the in-place replacement preserves the live pane"
    );
    assert_eq!(
        world.coverage.supervisor_recycles, 0,
        "the restart path owns this tick; it must not also run the recycle path"
    );
}

#[test]
fn restart_supervisor_on_fresh_binary_relaunches_not_reexecs() {
    // `#supkill-bg`: an explicit `restart-supervisor` on a FRESH binary has nothing to
    // upgrade, so `supervisor_restart_action` returns `RelaunchChild` at the boundary —
    // the normal kill-child → relaunch path, not an in-place reexec.
    let mut world = SimWorld::new(909);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    let gen_before = world.route.durable.generation;

    // Fresh binary (no MarkSupervisorBinaryStale), restart requested at an idle prompt.
    world.apply(SimCommand::RequestSupervisorRestart).unwrap();
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();

    assert_eq!(
        world.coverage.supervisor_restart_drain_reexecs, 0,
        "a fresh binary has nothing to hot-reload — no in-place reexec"
    );
    assert_eq!(
        world.coverage.supervisor_restart_relaunches, 1,
        "a fresh-binary restart relaunches the child"
    );
    assert!(!world.recycle_clear.restart_requested);
    assert_eq!(
        world.route.durable.generation, gen_before,
        "a child relaunch does not advance the supervisor generation in place"
    );
}

#[test]
fn restart_supervisor_reexec_failure_falls_back_to_relaunch() {
    // `#supkill-bg` / `#suprecyclestall`: if the in-place `execve` fails, the restart
    // must NOT strand the session — it clears the reexec intent and falls back to a
    // child relaunch on the current binary (pane survives, binary stays stale until a
    // clean restart succeeds).
    let mut world = SimWorld::new(1_337);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    let gen_before = world.route.durable.generation;

    world.apply(SimCommand::MarkSupervisorBinaryStale).unwrap();
    world.apply(SimCommand::MarkReexecWillFail).unwrap();
    world.apply(SimCommand::RequestSupervisorRestart).unwrap();
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();

    assert_eq!(
        world.coverage.supervisor_restart_drain_reexecs, 0,
        "a failed execve does not count as an in-place reexec"
    );
    assert_eq!(
        world.coverage.supervisor_restart_relaunches, 1,
        "the failed reexec falls back to a current-binary relaunch"
    );
    assert!(
        world.recycle_clear.binary_stale,
        "the failed reexec leaves the binary stale (operator restarts cleanly to upgrade)"
    );
    assert_eq!(world.route.durable.generation, gen_before);
}

#[test]
fn recyclerestart_agent_verifies_kill_pane_sync_guard_and_reclear_proofs() {
    // `#recyclerestart-agent`: replace the remaining live-only operator proof with
    // a deterministic model that covers both reported symptoms:
    // 1. recycle promotes the binary and the next queue item is re-cleared/drained;
    // 2. `Sync Tmux Layout` after a killed pane does not leave the plugin guard held forever.
    let mut world = SimWorld::new(6_150);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    let gen_before = world.route.durable.generation;
    let pane_before = world.route.durable.pane_id.clone();

    world.apply(SimCommand::MarkSupervisorBinaryStale).unwrap();
    world
        .apply(SimCommand::EnableSupervisorAutoRecycle)
        .unwrap();
    world.apply(SimCommand::OperatorRecycleMark).unwrap();
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();

    assert_eq!(world.coverage.supervisor_recycles, 1);
    assert_eq!(world.coverage.recycle_binary_promotion_proofs, 1);
    assert_eq!(world.route.durable.generation, gen_before + 1);
    assert_eq!(
        world.route.durable.pane_id, pane_before,
        "the hot reload must preserve the live pane"
    );

    world.apply(SimCommand::ActivateGoModeQueueHead).unwrap();
    world.apply(SimCommand::SessionClear).unwrap();
    for _ in 0..4 {
        world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    }
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();

    assert_eq!(world.coverage.recycle_session_reclear_proofs, 1);
    assert_eq!(world.coverage.clear_cooldown_resumes, 1);
    assert_eq!(world.coverage.go_drain_dispatches, 1);

    world
        .apply(SimCommand::StartBlockingSyncAfterKillPane)
        .unwrap();
    world.apply(SimCommand::SyncLayoutClick).unwrap();
    assert_eq!(
        world.coverage.sync_guard_defers, 1,
        "a legitimately fresh holder may defer one later click"
    );

    world
        .apply(SimCommand::AdvanceSyncGuardBeyondBound)
        .unwrap();
    world.apply(SimCommand::SyncLayoutClick).unwrap();
    assert_eq!(
        world.coverage.sync_guard_stale_releases, 1,
        "the stale-holder decision must supersede a wedged guard"
    );
    world.apply(SimCommand::FinishSyncLayout).unwrap();
    world.apply(SimCommand::SyncLayoutClick).unwrap();
    assert_eq!(
        world.coverage.sync_guard_completions, 1,
        "the superseded sync reaches the normal unlock path"
    );

    let ops_log = world.ops_log.join("\n");
    assert!(
        ops_log.contains("supervisor_recycle_action result=binary_promoted"),
        "ops log must distinguish binary promotion:\n{ops_log}"
    );
    assert!(
        ops_log.contains("recycle_session_reclear action=session_clear")
            && ops_log.contains("recycle_session_reclear action=idle_queue_drain result=dispatch"),
        "ops log must prove the post-recycle session clear/drain:\n{ops_log}"
    );
    assert!(
        ops_log.contains("sync_kill_pane_path pane_killed=true guard=held")
            && ops_log.contains("sync_guard_released reason=stale_holder_superseded")
            && ops_log.contains("sync_guard_released reason=sync_complete"),
        "ops log must prove killed-pane sync guard recovery:\n{ops_log}"
    );
}

#[test]
fn suprehotreload_agent_maps_jb_run_agent_doc_to_fresh_binary_proof() {
    // `#suprehotreload-agent`: a JB `Run Agent Doc` cycle can be verified without
    // live operator inspection when the stale supervisor reaches the recycle
    // boundary: the proof records a preserved-pane promotion onto the fresh binary.
    let mut world = SimWorld::new(6_151);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    let gen_before = world.route.durable.generation;
    let pane_before = world.route.durable.pane_id.clone();

    world.apply(SimCommand::MarkSupervisorBinaryStale).unwrap();
    world.apply(SimCommand::DispatchOperatorPrompt).unwrap();
    world.apply(SimCommand::ProveDispatchAccepted).unwrap();
    assert!(world.recycle_clear.jb_run_recycle_probe_pending);

    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();

    assert_eq!(world.coverage.supervisor_recycles, 1);
    assert_eq!(world.coverage.suprehot_jb_observed_promotions, 1);
    assert_eq!(world.coverage.suprehot_jb_mapped_recycle_failures, 0);
    assert!(!world.recycle_clear.jb_run_recycle_probe_pending);
    assert!(!world.recycle_clear.binary_stale);
    assert_eq!(world.route.durable.generation, gen_before + 1);
    assert_eq!(
        world.route.durable.pane_id, pane_before,
        "hot reload must preserve the JB-owned live pane"
    );

    let ops_log = world.ops_log.join("\n");
    assert!(
        ops_log
            .contains("suprehotreload_jb_cycle action=jb_run_agent_doc result=dispatch_accepted")
            && ops_log.contains("suprehotreload_jb_cycle result=observed_fresh_binary")
            && ops_log.contains("mapped=direct")
            && ops_log.contains("supervisor_recycle_action result=binary_promoted"),
        "ops log must prove the JB cycle observed a fresh binary:\n{ops_log}"
    );
}

#[test]
fn suprehotreload_agent_maps_reexec_failure_to_existing_operator_verify_buckets() {
    // `#suprehotreload-agent`: if execve fails, the agent-verifiable result is not
    // silent operator inspection. It maps the failed hot-reload attempt to the
    // existing live-proof buckets that already own recycle/restart verification.
    let mut world = SimWorld::new(6_152);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    let gen_before = world.route.durable.generation;
    let pane_before = world.route.durable.pane_id.clone();

    world.apply(SimCommand::MarkSupervisorBinaryStale).unwrap();
    world.apply(SimCommand::MarkReexecWillFail).unwrap();
    world.apply(SimCommand::DispatchOperatorPrompt).unwrap();
    world.apply(SimCommand::ProveDispatchAccepted).unwrap();
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();

    assert_eq!(world.coverage.supervisor_recycle_failures, 1);
    assert_eq!(world.coverage.suprehot_jb_observed_promotions, 0);
    assert_eq!(world.coverage.suprehot_jb_mapped_recycle_failures, 1);
    assert!(!world.recycle_clear.jb_run_recycle_probe_pending);
    assert!(world.recycle_clear.recycle_disabled);
    assert!(
        world.recycle_clear.binary_stale,
        "failed execve leaves the supervisor on the stale binary"
    );
    assert_eq!(world.route.durable.generation, gen_before);
    assert_eq!(
        world.route.durable.pane_id, pane_before,
        "failed recycle keeps the session alive in the same pane"
    );

    let ops_log = world.ops_log.join("\n");
    assert!(
        ops_log.contains(
            "suprehotreload_jb_cycle result=mapped_operator_verify targets=recyclerestart-verify,aazp,4myd"
        ) && ops_log.contains("reason=supervisor_reexec_failed"),
        "ops log must map failed hot reload to the existing proof buckets:\n{ops_log}"
    );
}

#[test]
fn opted_out_document_clears_and_drains_without_auto_recycle() {
    // `#simworld` / `#supselfheal`: a document that explicitly OPTED OUT of
    // `agent_doc_supervisor_auto_recycle` (falsey env/frontmatter/project knob) must
    // still perform the auto-CLEAR + go-mode drain steps between queue items, even
    // though its stale supervisor does NOT auto-recycle. This proves the recycle/clear
    // pipeline is independent of the recycle decision: with self-recycle now default-ON
    // (`#supselfheal`), an opt-OUT makes `supervisor_recycle_action` return
    // `Detect`/surface-only, but the clear-cooldown resume + drain is universal. Mirrors
    // `recycle_clear_pipeline_resumes_go_mode_drain_as_a_step` with auto-recycle forced
    // OFF to isolate the clear+drain steps from the recycle path.
    let mut world = SimWorld::new(7_311);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    // Explicit opt-out (default is now ON).
    world
        .apply(SimCommand::DisableSupervisorAutoRecycle)
        .unwrap();

    let gen_before = world.route.durable.generation;
    let pane_before = world.route.durable.pane_id.clone();

    // The supervisor binary is stale (a later `cargo install`), but this document opted
    // OUT of auto-recycle. A go-mode head is waiting and the operator `session clear`
    // (or the watch's deferred clear) wrote the manual clear cooldown.
    world.apply(SimCommand::MarkSupervisorBinaryStale).unwrap();
    world.apply(SimCommand::ActivateGoModeQueueHead).unwrap();
    world.apply(SimCommand::SessionClear).unwrap();
    assert!(world.recycle_clear.clear_cooldown_active);

    // Settle debounce: the cooldown must hold for CLEAR_COOLDOWN_RESUME_IDLE_TICKS
    // consecutive idle polls. Throughout, the stale binary must NOT recycle (auto-recycle
    // OFF → `Detect`/surface-only), so the generation/pane are untouched and the binary
    // stays stale — only an operator restart promotes the new build for a non-opted-in doc.
    for tick in 1..4 {
        world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
        assert!(
            world.recycle_clear.clear_cooldown_active,
            "cooldown must hold at settle tick {tick}"
        );
        assert_eq!(world.coverage.clear_cooldown_resumes, 0);
        assert_eq!(world.coverage.go_drain_dispatches, 0);
        assert_eq!(
            world.coverage.supervisor_recycles, 0,
            "an opted-out stale supervisor never auto-recycles"
        );
    }

    // The cooldown auto-expires after the settle debounce — the auto-CLEAR step ran.
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert!(
        !world.recycle_clear.clear_cooldown_active,
        "cooldown auto-expired after the settle debounce — the clear step ran"
    );
    assert_eq!(world.coverage.clear_cooldown_resumes, 1);
    assert_eq!(
        world.coverage.go_drain_dispatches, 0,
        "the resume tick re-evaluates; it does not dispatch into the just-cleared pane"
    );

    // Next tick: the go-mode drain dispatches the waiting head — the queue drains across
    // the clear with no auto-recycle involved.
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(
        world.coverage.go_drain_dispatches, 1,
        "the opted-out doc still drains its go-mode queue after the clear settles"
    );

    // The whole time, the stale supervisor stayed on its current binary: no recycle, no
    // generation churn, no cold pane rebind. Staleness is surfaced (Detect), not silently
    // hot-reloaded, for a document that opted out.
    assert_eq!(
        world.coverage.supervisor_recycles, 0,
        "opted out → the doc surfaces staleness instead of recycling"
    );
    assert_eq!(world.coverage.supervisor_recycle_failures, 0);
    assert!(
        world.recycle_clear.binary_stale,
        "the binary stays stale until an operator restart — no auto-recycle for an opted-out doc"
    );
    assert_eq!(
        world.route.durable.generation, gen_before,
        "no recycle → the generation is untouched"
    );
    assert_eq!(
        world.route.durable.pane_id, pane_before,
        "no recycle → the live pane is preserved (no cold rebind)"
    );
}

#[test]
fn focused_cycle_yields_to_supervisor_clear_and_fresh_agent_drain() {
    // `#qfocsup` / `#tb4q` / `#ftimmediate`: deterministic proof for the
    // operator-visible path. Pin the production queue classifier first so the
    // simulated yield -> clear -> dispatch schedule cannot pass while free-text
    // execution context is disconnected from the real drain seam.
    let free_text_head = "[focused-cycle] fix the free-text queue regression";
    let free_text_doc = format!(
        "---\nqueue_active: true\n---\n\n\
         <!-- agent:queue auto go -->\n- {free_text_head}\n<!-- /agent:queue -->\n"
    );
    assert_eq!(
        agent_doc_queue::queue_continuation::live_drainable_continuation_head(
            &free_text_doc,
            agent_doc_queue::queue_continuation::DrainScope::InSessionLoop,
        ),
        None,
        "the current session yields the tagged free-text head"
    );
    assert_eq!(
        agent_doc_queue::queue_continuation::live_drainable_continuation_head(
            &free_text_doc,
            agent_doc_queue::queue_continuation::DrainScope::Supervisor,
        )
        .as_deref(),
        Some(free_text_head),
        "the supervisor owns the same head after the clear"
    );
    assert!(
        agent_doc_queue::queue_continuation::head_requires_focused_cycle_in(
            &free_text_doc,
            free_text_head,
        )
    );

    // A focused-cycle head is not run by the accreted in-session loop. It yields
    // with `ui_outcome=deferred_for_supervisor_drain`, the supervisor promotes a
    // fresh binary if needed, force-clears for a fresh context, dispatches the
    // head, and the fresh agent consumes it with response-materialization proof.
    let mut world = SimWorld::new(8_416);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    world
        .apply(SimCommand::ActivateFocusedCycleQueueHead)
        .unwrap();

    let gen_before = world.route.durable.generation;
    let pane_before = world.route.durable.pane_id.clone();

    world
        .apply(SimCommand::SessionCheckFocusedCycleDeferredForSupervisorDrain)
        .unwrap();
    world.apply(SimCommand::MarkSupervisorBinaryStale).unwrap();
    world.apply(SimCommand::OperatorRecycleMark).unwrap();
    world
        .apply(SimCommand::SupervisorContextResetClear)
        .unwrap();

    for _ in 0..3 {
        world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
        assert_eq!(
            world.coverage.go_drain_dispatches, 0,
            "the drain must wait while the supervisor-owned /clear settles"
        );
    }

    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(
        world.coverage.supervisor_recycles, 1,
        "a stale supervisor promotes at the idle boundary before the drain"
    );
    assert_eq!(
        world.route.durable.generation,
        gen_before + 1,
        "in-place recycle advances generation"
    );
    assert_eq!(
        world.route.durable.pane_id, pane_before,
        "in-place recycle preserves the live pane"
    );
    assert_eq!(world.coverage.go_drain_dispatches, 1);

    world.apply(SimCommand::FreshAgentDrainActiveHead).unwrap();
    assert!(world.recycle_clear.queue_active_head.is_none());
    assert_eq!(world.coverage.focused_cycle_supervisor_yields, 1);
    assert_eq!(world.coverage.focused_cycle_context_resets, 1);
    assert_eq!(world.coverage.focused_cycle_fresh_agent_drains, 1);

    let ops_log = world.ops_log.join("\n");
    let yield_pos = ops_log
        .find("session_check_supervisor_drain_handoff")
        .expect("session-check handoff proof");
    let reset_pos = ops_log
        .find("idle_queue_watch_context_reset")
        .expect("context reset proof");
    let dispatch_pos = ops_log
        .find("proof=go_drain_dispatch")
        .expect("go-drain dispatch proof");
    let fresh_pos = ops_log
        .find("fresh_agent_drain_evidence")
        .expect("fresh-agent drain proof");
    assert!(
        yield_pos < reset_pos && reset_pos < dispatch_pos && dispatch_pos < fresh_pos,
        "proof markers must preserve the yield -> clear -> dispatch -> fresh-agent order:\n{ops_log}"
    );
    assert!(ops_log.contains("ui_outcome=deferred_for_supervisor_drain"));
    assert!(ops_log.contains("next_action=yield_to_supervisor_clear_and_continue"));
    assert!(ops_log.contains("#qfocsup"));
    assert!(ops_log.contains("response_materialized=true"));
}

#[test]
fn stale_supervisor_self_recycles_at_turn_boundary_by_default() {
    // `#supselfheal`: the headline behavior change. With NO opt-in command and NO
    // operator `admin recycle` mark, a stale supervisor must self-retire at the next
    // turn boundary via the blue/green `execve` hot-reload — because turn-boundary
    // self-recycle now defaults ON (`resolve_supervisor_auto_recycle` → true). This is
    // the hands-off self-heal for the freshly-`cargo install`ed-but-still-stale case
    // that otherwise re-files File Cache Conflict / IPC-drift dialogs forever
    // (`#fcc0`/`#ipcdrift`). The blue/green guarantee: the live pane is preserved (no
    // cold rebind) and only the generation advances.
    let mut world = SimWorld::new(7_313);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();

    let gen_before = world.route.durable.generation;
    let pane_before = world.route.durable.pane_id.clone();

    // The supervisor binary goes stale (a later `cargo install`). No opt-in, no
    // operator mark — the default-on policy must drive the recycle on its own.
    world.apply(SimCommand::MarkSupervisorBinaryStale).unwrap();
    assert!(
        world.recycle_clear.auto_recycle,
        "self-recycle defaults ON (`#supselfheal`) with no opt-in command"
    );

    // First idle tick at a turn boundary: stale + default-on → recycle in place. No head
    // is waiting, so the recycle does not dispatch, but the binary is promoted.
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(
        world.coverage.supervisor_recycles, 1,
        "the default-on policy self-recycles a stale supervisor — no opt-in, no operator mark"
    );
    assert_eq!(
        world.coverage.supervisor_recycle_failures, 0,
        "the blue/green execve hot-reload succeeded"
    );
    assert!(
        !world.recycle_clear.binary_stale,
        "self-recycle promoted the freshly-installed binary"
    );
    assert_eq!(
        world.route.durable.generation,
        gen_before + 1,
        "recycle advances the generation"
    );
    assert_eq!(
        world.route.durable.pane_id, pane_before,
        "blue/green: the live pane is preserved via execve (not a cold rebind)"
    );
}

#[test]
fn stale_supervisor_recycles_even_when_the_drain_bails_early() {
    // `#stalereexecstarve`: the regression this pins is CONTROL FLOW, not policy.
    //
    // The idle-queue watch does two unrelated jobs per tick: drain the go-mode
    // queue, and decide whether a stale supervisor should hot-reload onto the
    // freshly-installed binary. The drain has several legitimate early exits —
    // unavailable queue authority, an in-flight context clear, a settling clear
    // cooldown — and each one used to `continue`/`return` the WHOLE tick. The
    // recycle decision lives after the drain, so any supervisor stuck in one of
    // those drain states could never reach it.
    //
    // Observed live 2026-08-09 on `src/boost-client/tasks/monsterrodholders.md`:
    // PID 4069526 ran four days on a DELETED binary image
    // (`readlink /proc/4069526/exe` → `... (deleted)`) while its controller
    // projection stayed unavailable. 227 CP recycle requests were written and
    // none was ever consumed, and the ops log carried zero
    // `supervisor_binary_stale_*` lines for the whole window — the decision was
    // never reached, so it could not even report itself as deferred.
    //
    // Model the clear-cooldown-resume early exit (the drain-scoped bail that is
    // deterministic in SimWorld) with a stale binary and assert the recycle still
    // happens on that same tick.
    let mut world = SimWorld::new(9_041);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();

    let gen_before = world.route.durable.generation;
    let pane_before = world.route.durable.pane_id.clone();

    // A go-mode head is waiting behind a clear cooldown that is about to settle:
    // the drain will take its early exit on this tick.
    world.recycle_clear.queue_active_head = Some("do [#stalereexecstarve]".to_string());
    world.recycle_clear.clear_cooldown_active = true;
    // One short of the resume threshold: this tick's own idle accounting brings it
    // to the bar, so the drain takes its early exit inside the tick under test.
    world.recycle_clear.clear_cooldown_idle_ticks =
        agent_doc_queue::queue::CLEAR_COOLDOWN_RESUME_IDLE_TICKS.saturating_sub(1);

    // ...and the binary went stale underneath it (a later `cargo install`).
    world.apply(SimCommand::MarkSupervisorBinaryStale).unwrap();
    assert!(
        world.recycle_clear.binary_stale,
        "precondition: the supervisor maps the old binary"
    );

    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();

    // The drain DID take its early exit — that part is unchanged and correct.
    assert!(
        !world.recycle_clear.clear_cooldown_active,
        "the clear cooldown resumed, which is the drain-scoped early exit"
    );
    // ...and the recycle decision still ran on the SAME tick.
    assert_eq!(
        world.coverage.supervisor_recycles, 1,
        "a drain-scoped early exit must not starve the stale-binary recycle (#stalereexecstarve)"
    );
    assert!(
        !world.recycle_clear.binary_stale,
        "the execve hot-reload promoted the freshly-installed binary"
    );
    assert_eq!(
        world.route.durable.generation,
        gen_before + 1,
        "recycle advances the generation"
    );
    assert_eq!(
        world.route.durable.pane_id, pane_before,
        "blue/green: the live pane survives the hot-reload"
    );
}

#[test]
fn wedged_opted_out_supervisor_recycles_on_write_wedge() {
    // `#supselfheal` Phase 2 (`#supselfheal-wedgetrigger`): reproduces the firsthand
    // session — a stale-binary route-owned supervisor whose editor-IPC write is
    // wedged (repeated send_failed/no_ack against a nominally-active JB listener) so
    // an orphaned response cannot land. The doc OPTED OUT of auto-recycle, so without
    // the wedge trigger the policy would only `Detect`/surface and the wedge would be
    // indefinite. The wedge fact must override the default-OFF opt-out and recycle the
    // stale supervisor immediately at the turn boundary, clearing the wedge so the
    // previously-orphaned response commits — with NO operator restart and NO
    // `--force-disk`.
    let mut world = SimWorld::new(7_531);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    // Explicit opt-out: absent the wedge trigger this stale supervisor only surfaces.
    world
        .apply(SimCommand::DisableSupervisorAutoRecycle)
        .unwrap();

    let gen_before = world.route.durable.generation;
    let pane_before = world.route.durable.pane_id.clone();

    // Stale binary + a wedged editor-IPC write (the orphaned-response state).
    world.apply(SimCommand::MarkSupervisorBinaryStale).unwrap();
    world.apply(SimCommand::MarkWriteWedged).unwrap();
    assert!(world.recycle_clear.write_wedged);

    // One idle tick at a turn boundary: the wedge overrides the opt-out →
    // RecycleImmediate. The in-place execve promotes the fresh binary, preserves the
    // live pane, and clears the wedge.
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(
        world.coverage.wedge_triggered_recycles, 1,
        "a wedged write against an opted-OUT stale supervisor recycles via the wedge trigger"
    );
    assert_eq!(
        world.coverage.supervisor_recycles, 1,
        "the wedge-triggered recycle is a real in-place execve recycle"
    );
    assert!(
        !world.recycle_clear.write_wedged,
        "promoting the fresh binary clears the editor-IPC wedge"
    );
    assert!(
        !world.recycle_clear.binary_stale,
        "the wedge trigger promoted the freshly-installed binary"
    );
    assert_eq!(
        world.route.durable.generation,
        gen_before + 1,
        "the wedge-triggered recycle advances the generation"
    );
    assert_eq!(
        world.route.durable.pane_id, pane_before,
        "blue/green: the live pane is preserved (no cold rebind, no operator restart)"
    );
}

#[test]
fn failed_reexec_escalates_to_bounded_kill_relaunch() {
    // `#supselfheal` Phase 3 (`#supselfheal-reexecescalate`): when the in-place execve
    // re-exec cannot start (a deleted inode from a fresh `make install`, or another
    // syscall error), the supervisor must NOT sit on `continue_current_binary`
    // forever. After the failed re-exec, the recycle policy returns
    // `EscalateKillRelaunch`, and the watch escalates to a bounded kill+relaunch of
    // the harness child (reclaiming the wedged child so the orphaned response commits),
    // capped at MAX_REEXEC_ESCALATIONS.
    use agent_doc_supervisor::lifecycle::MAX_REEXEC_ESCALATIONS;

    let mut world = SimWorld::new(7_532);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();

    // Stale binary, a wedged write, and the next in-place execve WILL fail to launch.
    world.apply(SimCommand::MarkSupervisorBinaryStale).unwrap();
    world.apply(SimCommand::MarkWriteWedged).unwrap();
    world.apply(SimCommand::MarkReexecWillFail).unwrap();

    // Tick 1: stale + wedge → RecycleImmediate, but the execve fails. The watch records
    // the failure (`reexec_failed`) instead of promoting a binary.
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(
        world.coverage.supervisor_recycle_failures, 1,
        "the in-place execve failed"
    );
    assert!(
        world.recycle_clear.reexec_failed,
        "a failed execve marks the policy to escalate next tick"
    );
    assert_eq!(
        world.coverage.reexec_kill_relaunch_escalations, 0,
        "the escalation fires on the NEXT tick, once reexec_failed is set"
    );

    // Subsequent ticks: the policy returns EscalateKillRelaunch and the watch escalates
    // to a bounded kill+relaunch, capped at MAX_REEXEC_ESCALATIONS — never an
    // indefinite wedge. The kill+relaunch reclaims the wedged child so the orphaned
    // response can commit; the wedge clears.
    for _ in 0..(MAX_REEXEC_ESCALATIONS + 3) {
        world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    }
    assert_eq!(
        world.coverage.reexec_kill_relaunch_escalations as u32, MAX_REEXEC_ESCALATIONS,
        "kill+relaunch escalation is bounded at MAX_REEXEC_ESCALATIONS — no unbounded kill loop"
    );
    assert!(
        !world.recycle_clear.write_wedged,
        "the kill+relaunch reclaimed the wedged child so the orphaned response commits"
    );
}

#[test]
fn non_dogfood_document_auto_recycles_and_drains_from_opt_in_alone() {
    // `#simworld` / `#suprecyclecfg`: the COMPLEMENT of
    // `opted_out_document_clears_and_drains_without_auto_recycle`. A non-dogfooding
    // document whose project (or frontmatter) DID opt into
    // `agent_doc_supervisor_auto_recycle` must perform BOTH the auto-recycle AND the
    // auto-clear + go-mode drain steps, driven purely by the opt-in — with NO operator
    // `admin recycle` mark. This proves the per-document/per-project opt-in alone (the
    // resolution rung added in `resolve_supervisor_auto_recycle`) is sufficient to drive
    // the full recycle/clear/drain pipeline for any document, not just the dogfooding
    // repo and not requiring a manual `admin recycle --all-projects`. Mirrors
    // `recycle_clear_pipeline_resumes_go_mode_drain_as_a_step` but replaces the
    // `OperatorRecycleMark` with the auto path (stale binary + opt-in → the production
    // `supervisor_recycle_action` predicate fires on its own).
    let mut world = SimWorld::new(7_312);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();

    let gen_before = world.route.durable.generation;
    let pane_before = world.route.durable.pane_id.clone();

    // The supervisor binary is stale (a later `cargo install`) and this document opted
    // into auto-recycle — but the operator never ran `admin recycle`. The opt-in alone
    // must drive the recycle.
    world.apply(SimCommand::MarkSupervisorBinaryStale).unwrap();
    world
        .apply(SimCommand::EnableSupervisorAutoRecycle)
        .unwrap();

    // First idle tick: stale + opt-in + turn boundary → `supervisor_recycle_action`
    // returns Recycle (no operator mark needed). The in-place `execve` promotes the
    // fresh binary, preserves the live pane, and advances the generation. No head is
    // waiting yet, so the recycle does not dispatch.
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(
        world.coverage.supervisor_recycles, 1,
        "the opt-in alone drives the auto-recycle — no operator `admin recycle` mark"
    );
    assert_eq!(
        world.route.durable.generation,
        gen_before + 1,
        "recycle advances the generation"
    );
    assert_eq!(
        world.route.durable.pane_id, pane_before,
        "recycle preserves the live pane via execve (not a cold rebind)"
    );
    assert!(
        !world.recycle_clear.binary_stale,
        "recycle promoted the freshly-installed binary"
    );
    assert_eq!(
        world.coverage.go_drain_dispatches, 0,
        "no head was waiting at the recycle boundary"
    );

    // A go-mode head is now waiting; the operator `session clear` records the
    // manual clear-cooldown projection before any idle poll observes it.
    world.apply(SimCommand::ActivateGoModeQueueHead).unwrap();
    world.apply(SimCommand::SessionClear).unwrap();
    assert!(world.recycle_clear.clear_cooldown_active);

    // Settle debounce: the cooldown holds for CLEAR_COOLDOWN_RESUME_IDLE_TICKS
    // consecutive idle polls. The binary is already fresh, so NO further recycle fires.
    for tick in 1..4 {
        world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
        assert!(
            world.recycle_clear.clear_cooldown_active,
            "cooldown must hold at settle tick {tick}"
        );
        assert_eq!(world.coverage.clear_cooldown_resumes, 0);
        assert_eq!(world.coverage.go_drain_dispatches, 0);
        assert_eq!(
            world.coverage.supervisor_recycles, 1,
            "a freshly-recycled supervisor does not recycle again"
        );
    }

    // 4th settled idle tick: the auto-CLEAR step completes (cooldown auto-expires).
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert!(
        !world.recycle_clear.clear_cooldown_active,
        "cooldown auto-expired after the settle debounce — the clear step ran"
    );
    assert_eq!(world.coverage.clear_cooldown_resumes, 1);
    assert_eq!(
        world.coverage.go_drain_dispatches, 0,
        "the resume tick re-evaluates; it does not dispatch into the just-cleared pane"
    );

    // Next tick: the go-mode drain dispatches the waiting head — recycle + clear + drain
    // all resumed as steps, driven by the auto-recycle opt-in with no operator route.
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(
        world.coverage.go_drain_dispatches, 1,
        "the non-dogfooding doc drains its go-mode queue after the opt-in recycle + clear"
    );
    assert_eq!(
        world.coverage.supervisor_recycle_failures, 0,
        "the in-place recycle succeeded"
    );

    // A stuck head is not re-fired every tick (drain dedup).
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(world.coverage.go_drain_dispatches, 1);
}

#[test]
fn failed_reexec_recycle_keeps_session_alive_and_does_not_retry() {
    // `#suprecyclestall`: a stale binary opted into auto-recycle, but the in-place
    // `execve` fails (no candidate binary launches). The watch must NOT
    // `process::exit` — that orphaned the live child and hung the tmux pane.
    // Instead it logs, disables further recycle attempts, and keeps running on the
    // current binary; the operator restarts deliberately.
    let mut world = SimWorld::new(2_026);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();

    let gen_before = world.route.durable.generation;
    let pane_before = world.route.durable.pane_id.clone();

    world.apply(SimCommand::MarkSupervisorBinaryStale).unwrap();
    world
        .apply(SimCommand::EnableSupervisorAutoRecycle)
        .unwrap();
    world.apply(SimCommand::MarkReexecWillFail).unwrap();

    // Idle boundary: the recycle is attempted and fails.
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(
        world.coverage.supervisor_recycle_failures, 1,
        "the failed execve recycle is recorded"
    );
    assert_eq!(
        world.coverage.supervisor_recycles, 0,
        "no successful in-place recycle happened"
    );
    assert_eq!(
        world.route.durable.pane_id, pane_before,
        "the session/pane survives a failed recycle (no process::exit)"
    );
    assert_eq!(
        world.route.durable.generation, gen_before,
        "a failed recycle does not advance the generation"
    );
    assert!(
        matches!(world.route.durable.lifecycle, SupervisorLifecycle::Ready),
        "the supervisor keeps running on its current binary"
    );
    assert!(
        world.recycle_clear.binary_stale,
        "the binary is still stale — only an operator restart promotes the new build"
    );

    // Subsequent idle boundaries do NOT re-attempt the hopeless recycle.
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(
        world.coverage.supervisor_recycle_failures, 1,
        "recycle is disabled after the first failure — no per-tick re-spam"
    );
}

#[test]
fn qflood2_drain_holds_trigger_until_own_clear_settles() {
    // `#qflood2`: after the idle-queue watch sends its OWN `/clear` between queue
    // items, the next drain trigger must NOT dispatch until the cleared pane has
    // settled for CLEAR_COOLDOWN_RESUME_IDLE_TICKS consecutive idle polls.
    // Otherwise it lands in the still-in-flight clear and the harness sees one
    // concatenated line (`/clear /agent-doc <FILE>`). Driven by the production
    // `drain_blocked_awaiting_clear_settle` predicate.
    let mut world = SimWorld::new(4_242);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    world.apply(SimCommand::ActivateGoModeQueueHead).unwrap();

    // The watch sends its own `/clear` (no manual cooldown projection recorded).
    world
        .apply(SimCommand::SupervisorContextResetClear)
        .unwrap();

    // The first 3 settle ticks must HOLD the trigger (drain_settle_skip), never
    // dispatching into the in-flight clear.
    for tick in 1..4 {
        world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
        assert_eq!(
            world.coverage.go_drain_dispatches, 0,
            "trigger must not dispatch into the in-flight clear at settle tick {tick}"
        );
        assert_eq!(world.coverage.drain_settle_skips, tick);
    }

    // 4th settled idle tick: the gate releases AND the drain dispatches the head
    // exactly once — one `/clear`, then one trigger, with no concatenation.
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(
        world.coverage.go_drain_dispatches, 1,
        "the single trigger dispatches once the pane settled after the clear"
    );

    // It is not re-fired on subsequent ticks (drain dedup on the same head).
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(world.coverage.go_drain_dispatches, 1);
}

#[test]
fn qflood2_drain_dedups_trigger_already_pending_in_composer() {
    // `#qflood2`: even at a settled idle boundary, if the routed trigger is
    // already pending in the composer (a prior dispatch / another owner put it
    // there), the watch must skip the re-send so duplicates never stack
    // (`/agent-doc <FILE>/agent-doc <FILE>`). Driven by `drain_dispatch_dedup_skip`.
    let mut world = SimWorld::new(4_243);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    world.apply(SimCommand::ActivateGoModeQueueHead).unwrap();
    world
        .apply(SimCommand::SetTriggerAlreadyPending(true))
        .unwrap();

    // The pane is idle and a head is waiting, but the trigger is already pending:
    // the drain must dedup-skip, not stack a second copy.
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(
        world.coverage.go_drain_dispatches, 0,
        "a trigger already in the composer must not be re-sent"
    );
    assert_eq!(world.coverage.drain_dedup_skips, 1);

    // Once the composer clears (operator submitted / consumed the pending trigger),
    // the drain dispatches normally — the dedup never permanently suppresses it.
    world
        .apply(SimCommand::SetTriggerAlreadyPending(false))
        .unwrap();
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(
        world.coverage.go_drain_dispatches, 0,
        "the same head was already recorded as dispatched by the dedup-skip"
    );
    // The head was marked last_dispatched by the dedup path, so the normal drain
    // dedup (`SkipAlreadyDispatched`) now owns it — no duplicate, no hot loop.
    assert_eq!(world.coverage.drain_dedup_skips, 1);
}

/// `#brtc` / `#queuestatemachine3`: a deterministic stale-CRDT / supervisor
/// re-emit **storm**. The same identities are re-injected many times across
/// simulated cycles, mixed across every duplication shape the historical ad-hoc
/// dedup passes each patched — pin-variant `do [#id]` (`#qdedupsync` /
/// `#pushpinaccum`), bare `[#id]` mirror references (`#qdup-bare-id`),
/// multiline phantom pins (`#rt83qflood`), and operator free-text re-emits
/// (`#qauthorder`). Driving the production convergence
/// (`queue::converge_queue_via_lifecycle`, the unified SM-driven pass from
/// `#cgfx`) over the storm must converge to **exactly one item per identity**,
/// and an operator position-locked free-text line must keep its authored slot
/// across the whole storm.
///
/// This exercises the same production convergence path preflight queue
/// maintenance uses, modeled without a live editor/tmux.
#[test]
fn brtc_reemit_storm_converges_to_one_item_per_identity_and_preserves_operator_position() {
    use agent_doc_queue::document_queue::{self as queue, QueueEntry, QueuePrompt};

    fn pr(text: &str) -> QueueEntry {
        QueueEntry::Prompt(QueuePrompt {
            text: text.to_string(),
            multiline: false,
            indent: 0,
            ordered_marker: None,
        })
    }
    fn multi(text: &str) -> QueueEntry {
        QueueEntry::Prompt(QueuePrompt {
            text: text.to_string(),
            multiline: true,
            indent: 0,
            ordered_marker: None,
        })
    }

    // The operator-authored snapshot: two id heads with a position-locked
    // free-text line wedged between them, plus one phantom-pin block authored
    // once. This is the lawful baseline the storm must converge back to.
    let operator_line = "do not bubble my queue items to the top";
    let phantom = ":round_pushpin: switch actor\nroute error: pane busy";
    let snapshot = vec![
        pr("do [#alpha]"),
        pr(operator_line),
        pr("do [#beta]"),
        multi(phantom),
    ];

    // Build the storm: start from the snapshot, then have a stale CRDT /
    // supervisor re-emit each identity under MANY shapes across several cycles.
    let mut stormed = snapshot.clone();
    let mut rng = DeterministicRng::new(0xB17C);
    for _cycle in 0..6 {
        // Re-emit alpha under pin + bare-reference + bare-directive variants.
        let alpha_variants = [
            ":pushpin: do [#alpha]",
            "do [#alpha]",
            "[#alpha]",
            "#alpha",
            "_prioritized_ do [#alpha]",
        ];
        let beta_variants = [":pushpin: do [#beta]", "do [#beta]", "[#beta]", "#beta"];
        // Inject a deterministic but shuffled subset each cycle.
        for _ in 0..3 {
            stormed.push(pr(alpha_variants[rng.next_usize(alpha_variants.len())]));
            stormed.push(pr(beta_variants[rng.next_usize(beta_variants.len())]));
            // Free-text re-emit (#qauthorder) + phantom-pin flood (#rt83qflood).
            stormed.push(pr(operator_line));
            stormed.push(multi(phantom));
        }
    }
    assert!(
        stormed.len() > 50,
        "storm should be large: {} entries",
        stormed.len()
    );

    // Drive the production convergence to a fixpoint (a multi-cycle preflight
    // would re-run it each cycle; converging to a fixpoint models that).
    let mut converged = stormed.clone();
    let mut passes = 0;
    while let Some(next) =
        queue::converge_queue_via_lifecycle(&converged, &snapshot, &Default::default())
    {
        converged = next;
        passes += 1;
        assert!(passes < 10, "convergence must reach a fixpoint quickly");
    }
    // Idempotent: one more pass is a guaranteed no-op.
    assert!(
        queue::converge_queue_via_lifecycle(&converged, &snapshot, &Default::default()).is_none(),
        "converged queue must be a fixpoint:\n{converged:?}"
    );

    // EXACTLY ONE item per identity. Count live prompt heads by normalized id /
    // text key.
    let count_id = |id: &str| {
        converged
            .iter()
            .filter(|e| {
                matches!(e, QueueEntry::Prompt(p) if {
                    use agent_doc_element_queue::QueueItemIdentity;
                    QueueItemIdentity::from_prompt(&p.text)
                        == QueueItemIdentity::Id(id.to_string())
                })
            })
            .count()
    };
    assert_eq!(
        count_id("alpha"),
        1,
        "alpha must converge to one head:\n{converged:?}"
    );
    assert_eq!(
        count_id("beta"),
        1,
        "beta must converge to one head:\n{converged:?}"
    );

    let free_text_count = converged
        .iter()
        .filter(|e| matches!(e, QueueEntry::Prompt(p) if !p.multiline && p.text == operator_line))
        .count();
    assert_eq!(
        free_text_count, 1,
        "operator free-text line must converge to its authored count of one:\n{converged:?}"
    );
    let phantom_count = converged
        .iter()
        .filter(|e| matches!(e, QueueEntry::Prompt(p) if p.multiline))
        .count();
    assert_eq!(
        phantom_count, 1,
        "phantom-pin flood must converge to its authored count of one:\n{converged:?}"
    );

    // OPERATOR POSITION-LOCK survives the storm: the operator's free-text line
    // stays between the two id heads exactly as authored — never bubbled to the
    // top, never sunk below.
    let texts: Vec<String> = converged
        .iter()
        .filter_map(|e| match e {
            QueueEntry::Prompt(p) if !p.multiline => Some(p.text.clone()),
            _ => None,
        })
        .collect();
    let alpha_pos = texts
        .iter()
        .position(|t| {
            use agent_doc_element_queue::QueueItemIdentity;
            QueueItemIdentity::from_prompt(t) == QueueItemIdentity::Id("alpha".into())
        })
        .expect("alpha head present");
    let op_pos = texts
        .iter()
        .position(|t| t == operator_line)
        .expect("operator line present");
    let beta_pos = texts
        .iter()
        .position(|t| {
            use agent_doc_element_queue::QueueItemIdentity;
            QueueItemIdentity::from_prompt(t) == QueueItemIdentity::Id("beta".into())
        })
        .expect("beta head present");
    assert!(
        alpha_pos < op_pos && op_pos < beta_pos,
        "operator position-lock must hold across the storm: alpha={alpha_pos} op={op_pos} beta={beta_pos}\n{texts:?}"
    );
}

#[test]
fn qdedup_between_turn_enqueue_waits_for_idle_and_dedupes_command_set() {
    // `#qdedup`: repeated supervisor/CP between-turn handoff requests should be
    // buffered while a turn is active, then composed as a set at the idle boundary:
    // exactly one `/clear`, exactly one `agent-doc <FILE>`, in that order.
    let mut world = SimWorld::new(4_244);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorBusy).unwrap();
    world
        .apply(SimCommand::QueueBetweenTurnFreshContextHandoff)
        .unwrap();
    world
        .apply(SimCommand::QueueBetweenTurnFreshContextHandoff)
        .unwrap();

    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(
        world.coverage.between_turn_enqueue_deliveries, 0,
        "active turns must hold the buffered handoff"
    );
    assert_eq!(world.coverage.between_turn_enqueue_busy_skips, 1);

    world.apply(SimCommand::SupervisorReady).unwrap();
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(world.coverage.between_turn_enqueue_deliveries, 1);
    assert_eq!(
        world.coverage.between_turn_enqueue_deduped, 2,
        "two repeated handoffs contain four raw commands but deliver only two"
    );
    let ops_log = world.ops_log.join("\n");
    assert!(
        ops_log.contains("between_turn_enqueue deduped=2 kept=/clear,/agent-doc result=delivered"),
        "ops log must prove the deduped between-turn delivery:\n{ops_log}"
    );
}

#[test]
fn clear_cooldown_without_active_head_stays_authoritative() {
    // A plain operator clear with NO active go-mode head keeps the cooldown
    // authoritative-until-operator-route: the non-go behavior must be preserved.
    let mut world = SimWorld::new(2_027);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    world.apply(SimCommand::SessionClear).unwrap();
    assert!(world.recycle_clear.clear_cooldown_active);

    for _ in 0..8 {
        world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    }
    assert!(
        world.recycle_clear.clear_cooldown_active,
        "no active head → cooldown stays authoritative (non-go behavior preserved)"
    );
    assert_eq!(world.coverage.clear_cooldown_resumes, 0);
    assert_eq!(world.coverage.go_drain_dispatches, 0);
}

#[test]
fn clear_cooldown_deferred_operator_clear_blocks_resume() {
    // When an operator-deferred clear is still pending but the pane is busy, that
    // path owns its own resume — the cooldown auto-expiry must defer instead of
    // resuming the queue underneath an in-flight turn.
    let mut world = SimWorld::new(2_028);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    world.apply(SimCommand::ActivateGoModeQueueHead).unwrap();
    world.apply(SimCommand::DeferOperatorClearPending).unwrap();
    world.apply(SimCommand::SessionClear).unwrap();
    world.apply(SimCommand::SupervisorBusy).unwrap();

    for _ in 0..8 {
        world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    }
    assert!(
        world.recycle_clear.clear_cooldown_active,
        "a pending deferred operator clear blocks the auto-resume"
    );
    assert_eq!(world.coverage.clear_cooldown_resumes, 0);
}

#[test]
fn deferred_operator_clear_settles_before_resuming_drain() {
    // A deferred operator clear runs in the inter-turn idle gap. After it lands,
    // the next queue trigger must wait for the same clear-settle debounce as the
    // watch's own context-reset clears; otherwise `/clear` and the next drain
    // command can concatenate and the old context keeps accumulating.
    let mut world = SimWorld::new(2_030);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    world.apply(SimCommand::ActivateGoModeQueueHead).unwrap();
    world.apply(SimCommand::DeferOperatorClearPending).unwrap();
    world.apply(SimCommand::SessionClear).unwrap();

    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert!(
        !world.recycle_clear.deferred_operator_clear_pending,
        "the idle gap delivers the deferred clear"
    );
    assert!(
        !world.recycle_clear.clear_cooldown_active,
        "delivery clears the deferred clear pause signal"
    );
    assert!(
        world.recycle_clear.awaiting_clear_settle,
        "delivery must engage the in-flight-clear settle gate"
    );
    assert_eq!(world.coverage.go_drain_dispatches, 0);

    for tick in 1..4 {
        world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
        assert_eq!(
            world.coverage.go_drain_dispatches, 0,
            "drain must wait while the deferred clear settles (tick {tick})"
        );
    }

    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert!(
        !world.recycle_clear.awaiting_clear_settle,
        "the settle gate releases after the debounce window"
    );
    assert_eq!(
        world.coverage.go_drain_dispatches, 1,
        "the queue resumes only after the deferred clear settles"
    );
}

#[test]
fn repeated_busy_session_clear_keeps_single_deferred_clear() {
    // #p6a0: repeated non-interrupting clears during an active turn must not
    // inject or queue multiple `/clear` commands. The first request records the
    // supervisor handoff; later requests are idempotent until the idle boundary.
    let mut world = SimWorld::new(2_031);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    world.apply(SimCommand::ActivateGoModeQueueHead).unwrap();
    world.apply(SimCommand::SupervisorBusy).unwrap();

    world.apply(SimCommand::SessionClear).unwrap();
    assert!(world.recycle_clear.deferred_operator_clear_pending);
    assert!(world.recycle_clear.clear_cooldown_active);
    assert_eq!(world.coverage.session_clears, 1);

    world.apply(SimCommand::SessionClear).unwrap();
    world.apply(SimCommand::SessionClear).unwrap();
    assert!(world.recycle_clear.deferred_operator_clear_pending);
    assert_eq!(
        world.coverage.session_clears, 1,
        "duplicate clears while the turn is active must not rearm the clear"
    );
    assert_eq!(world.coverage.deferred_clear_duplicate_suppressed, 2);
    assert_eq!(world.coverage.recycle_session_reclear_proofs, 1);

    world.apply(SimCommand::SupervisorReady).unwrap();
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert!(
        !world.recycle_clear.deferred_operator_clear_pending,
        "the single queued clear is delivered at the next idle boundary"
    );
    assert!(world.recycle_clear.awaiting_clear_settle);
    assert_eq!(
        world.coverage.recycle_session_reclear_proofs, 2,
        "one proof for queued handoff, one proof for delivery"
    );
}

#[test]
fn clear_cooldown_resume_debounce_resets_when_a_turn_interrupts_idle() {
    // The settle debounce is consecutive: a turn becoming active mid-settle resets the
    // idle-tick count, so a resumed drain never injects into an in-flight turn.
    let mut world = SimWorld::new(2_029);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    world.apply(SimCommand::ActivateGoModeQueueHead).unwrap();
    world.apply(SimCommand::SessionClear).unwrap();

    // Two settle ticks, then a turn interrupts (Busy), resetting the debounce.
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    world.apply(SimCommand::SupervisorBusy).unwrap();
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(world.coverage.clear_cooldown_resumes, 0);
    assert!(world.recycle_clear.clear_cooldown_active);

    // Back to idle: must settle for the full threshold again from zero.
    world.apply(SimCommand::SupervisorReady).unwrap();
    for _ in 0..3 {
        world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
        assert!(world.recycle_clear.clear_cooldown_active);
    }
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(
        world.coverage.clear_cooldown_resumes, 1,
        "the debounce restarts from zero after the interrupting turn"
    );
}

#[test]
fn route_sim_blocks_starting_actor_that_closes_before_ready_prompt() {
    let mut world = SimWorld::new(2_007);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(world.coverage.route_dispatch_acceptances, 0);

    world.apply(SimCommand::SupervisorClosed).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();

    assert_eq!(world.coverage.closed_dispatch_blocks, 1);
    assert_eq!(world.coverage.starting_prompt_promotions, 0);
    assert_eq!(world.coverage.route_dispatch_acceptances, 0);
    assert_eq!(world.coverage.route_dispatch_proofs, 0);
}

#[test]
fn route_sim_protected_prompt_refusal_has_no_dispatch_churn() {
    let mut world = SimWorld::new(2_047);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();

    world
        .apply(SimCommand::DispatchOperatorPromptWithProtectedDraft)
        .unwrap();
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    world.apply(SimCommand::ProveDispatchAccepted).unwrap();

    assert_eq!(world.coverage.protected_prompt_route_blocks, 1);
    assert_eq!(world.coverage.route_dispatch_acceptances, 0);
    assert_eq!(world.coverage.route_dispatch_proofs, 0);
    assert_eq!(world.coverage.dispatch_injects, 0);
    let ops_log = world.ops_log.join("\n");
    assert!(
        ops_log.contains("route_dispatch_direct_pane_blocked")
            && ops_log.contains("protected_input=drafted prompt input")
            && ops_log.contains("draft_preview=\"› implement the feature\""),
        "protected-prompt refusal must be actionable without dispatch churn:\n{ops_log}"
    );
}

#[test]
fn jetbrains_clear_session_sim_keeps_starting_route_guard_and_repairs_prompt_duplicate() {
    let temp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(temp.path().join(".agent-doc/logs")).unwrap();
    let doc_path = temp.path().join("jetbrains-clear-session.md");

    let mut world = SimWorld::new(2_008);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::EditPrompt).unwrap();
    let before_ipc = world.doc.clone();

    world.apply(SimCommand::SessionClear).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();

    assert_eq!(world.coverage.session_clears, 1);
    assert_eq!(
        world.coverage.starting_dispatch_blocks, 1,
        "Run Agent Doc after clear must stay prompt-gated while the actor is still starting"
    );
    assert_eq!(world.coverage.route_dispatch_acceptances, 0);

    world
        .append_to_exchange("do #sim1. spec-test-build-install-commit-push\n")
        .unwrap();
    world.apply(SimCommand::CaptureResponse).unwrap();
    world.apply(SimCommand::ApplyCapturedResponse).unwrap();
    world
        .repair_ipc_snapshot_duplicate_prompts(&before_ipc, &doc_path)
        .unwrap();

    assert_eq!(world.coverage.prompt_duplicate_repairs, 1);
    assert_eq!(
        world
            .component_content("exchange")
            .unwrap()
            .matches("do #sim1. spec-test-build-install-commit-push")
            .count(),
        1,
        "IPC snapshot repair should keep the normalized live prompt once"
    );

    world.apply(SimCommand::Commit).unwrap();
    assert_eq!(world.phase, CyclePhase::Committed);
    assert_eq!(world.snapshot, world.doc);
}

#[test]
fn full_content_source_proof_sim_rejects_stale_editor_buffers() {
    for source in FullContentReplacementSource::ALL {
        let mut world = SimWorld::new(2_010);
        let original = world.doc.clone();

        let decision = world.stale_full_content_visible_replacement(source);

        assert_eq!(
            decision,
            agent_doc_document_realtime::write_policy::FullContentVisibleReplacementDecision::RejectStaleSourceBuffer,
            "{source:?} must reject full-content replacement when the editor buffer drifted"
        );
        assert!(
            world
                .doc
                .contains("❯ live prompt typed before full-content apply"),
            "{source:?} must preserve live editor text on stale proof rejection"
        );
        assert_ne!(
            world.doc, original,
            "{source:?} should model live prompt drift before the full-content apply attempt"
        );
        assert_eq!(
            world.coverage.stale_source_buffer_skips, 1,
            "{source:?} should count the stale full-content replacement skip"
        );
        assert!(
            !world
                .doc
                .contains(&format!("replacement from {}", source.as_str())),
            "{source:?} must not apply stale compact/repair/timeout replacement content"
        );
    }
}

#[test]
fn reconnect_buffer_sim_rereads_stale_then_keeps_user_edits() {
    use agent_doc_document_realtime::write_policy::{
        ReconnectBufferDecision, decide_reconnect_buffer,
    };
    let mut world = SimWorld::new(2_044);

    // The content the editor buffer last saw before the plugin disconnected.
    let prior_committed = world.doc.clone();

    // While disconnected, the binary committed a control-plane edit (queue/status
    // bookkeeping). Disk == HEAD now holds that newer content.
    world
        .append_to_exchange("<!-- agent:boundary:reconnect -->\n")
        .unwrap();
    let disk_head = world.doc.clone();
    assert_ne!(prior_committed, disk_head);

    // Case 1: the buffer is exactly the prior committed version (stale, unedited).
    // disk is clean HEAD, buffer matches a prior commit → re-read disk.
    let buffer_stale = prior_committed.clone();
    let decision = decide_reconnect_buffer(
        buffer_stale == disk_head,
        true, // disk == HEAD
        buffer_stale == prior_committed,
    );
    assert_eq!(
        decision,
        ReconnectBufferDecision::RereadDisk,
        "a buffer equal to a prior commit while disk is clean HEAD must re-read disk"
    );
    if decision == ReconnectBufferDecision::RereadDisk {
        world.coverage.reconnect_reread_decisions += 1;
    }

    // Case 2: the buffer has genuine unsynced user edits (matches neither disk
    // nor any prior commit) → editor wins, never clobber.
    let buffer_user_edit = format!("{prior_committed}\n❯ user typed offline\n");
    let decision = decide_reconnect_buffer(
        buffer_user_edit == disk_head,
        true,
        buffer_user_edit == prior_committed,
    );
    assert_eq!(
        decision,
        ReconnectBufferDecision::KeepBuffer,
        "a buffer with genuine user edits must be kept (editor wins per #editorbufwin)"
    );

    // Case 3: buffer already matches disk → no-op.
    let decision = decide_reconnect_buffer(true, true, true);
    assert_eq!(decision, ReconnectBufferDecision::InSync);

    assert_eq!(
        world.coverage.reconnect_reread_decisions, 1,
        "exactly one stale buffer should be re-read in this scenario"
    );
}

#[test]
fn editorless_cli_sim_uses_detached_disk_and_live_editor_fail_closed() {
    use agent_doc_document_realtime::write_policy::{
        EditorlessDiskFallbackDecision, decide_editorless_disk_fallback,
    };
    let mut world = SimWorld::new(2_046);
    world.append_to_exchange("❯ finalize me\n").unwrap();
    let threshold = 3;

    // #kcb5 realtime cutover: a CLI-only actor with a connectable controller
    // socket but no editor endpoint may use the guarded DetachedDisk path once
    // repeated no-ACKs prove there is no editor delivery.
    let cli_unforced = decide_editorless_disk_fallback(true, false, threshold, threshold, false);
    assert_eq!(
        cli_unforced,
        EditorlessDiskFallbackDecision::DetachedDisk,
        "an editor-less CLI actor with no delivery proof should use the guarded detached disk path"
    );
    if cli_unforced == EditorlessDiskFallbackDecision::DetachedDisk {
        world.coverage.editorless_disk_fallbacks += 1;
    }

    let cli = decide_editorless_disk_fallback(true, false, threshold, threshold, true);
    assert_eq!(
        cli,
        EditorlessDiskFallbackDecision::ForceDiskNoEditor,
        "an editor-less CLI actor routes finalize to disk only with explicit force_disk"
    );
    if cli == EditorlessDiskFallbackDecision::ForceDiskNoEditor {
        world.coverage.editorless_disk_fallbacks += 1;
    }

    // A live editor actor with the same failing delivery must STILL fail closed
    // (no regression of the editor-buffer / #editorbufwin protection).
    let live = decide_editorless_disk_fallback(true, true, threshold, threshold, false);
    assert_eq!(
        live,
        EditorlessDiskFallbackDecision::FailClosed,
        "a live editor buffer must never be disk-clobbered on unproven delivery"
    );

    assert_eq!(
        world.coverage.editorless_disk_fallbacks, 2,
        "both editor-less disk-authorized paths should be counted"
    );
}

#[test]
fn ipc_snapshot_adoption_sim_blocks_live_prompt_drift_after_preflight() {
    let mut world = SimWorld::new(2_021);
    world.append_to_exchange("❯ Please reply\n").unwrap();
    let baseline = world.doc.clone();
    let content_ours = baseline.replace(
        "<!-- /agent:exchange -->",
        "### Re: Please reply — gpt-5\n\nAnswered.\n<!-- /agent:exchange -->",
    );
    let snapshot_candidate = baseline.replace(
        "<!-- /agent:exchange -->",
        "❯ New prompt typed during closeout\n### Re: Please reply — gpt-5\n\nAnswered.\n<!-- /agent:exchange -->",
    );

    world.adopt_ipc_snapshot_candidate(&baseline, &content_ours, &snapshot_candidate);

    assert_eq!(world.coverage.ipc_snapshot_live_prompt_blocks, 1);
    assert_eq!(
        world.snapshot, content_ours,
        "IPC snapshot adoption must keep the committed snapshot to agent-owned response content"
    );
    assert!(
        world.doc.contains("❯ New prompt typed during closeout"),
        "the live prompt remains visible for the next response cycle"
    );
    assert!(
        !world.snapshot.contains("New prompt typed during closeout"),
        "the live prompt must not be absorbed into the snapshot"
    );
    assert_eq!(
        world
            .doc
            .matches("❯ New prompt typed during closeout")
            .count(),
        1,
        "the live prompt must remain exactly once"
    );
    assert_eq!(
        world.doc.matches("### Re: Please reply — gpt-5").count(),
        1,
        "the response heading must not be duplicated"
    );
}

// -------- #fintol3: finalize tolerance for independent concurrent edits --------

#[test]
fn finalize_forward_merges_plain_concurrent_edit_outside_response() {
    // #fintol3: the operator edits a plain parked comment-note OUTSIDE the
    // response target while the agent finalizes. The edit carries no
    // prompt/directive, so finalize forward-merges instead of rejecting — the
    // response commits AND the user's edit is preserved in the same commit.
    let mut world = SimWorld::new(2_044);
    let baseline = concat!(
        "---\nsession: test\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "old parked note\n",
        "<!-- /agent:exchange -->\n\n",
        "<!--\nold parked note body\n-->\n",
    );
    let content_ours = baseline.replace(
        "<!-- /agent:exchange -->",
        "### Re: reply — gpt-5\n\nAnswered with a sufficiently long response body.\n<!-- /agent:exchange -->",
    );
    // The user edits the parked note prose (outside exchange) mid-finalize.
    let candidate = content_ours.replace("old parked note body", "edited parked note body");

    world.finalize_ipc_candidate_with_tolerance(baseline, &content_ours, &candidate);

    assert_eq!(
        world.coverage.live_prompt_forward_merges, 1,
        "a disjoint plain edit outside the response must forward-merge"
    );
    assert_eq!(
        world.coverage.ipc_snapshot_live_prompt_blocks, 0,
        "a disjoint plain edit must NOT be rejected"
    );
    assert!(
        world.snapshot.contains("### Re: reply — gpt-5"),
        "the agent response must be committed:\n{}",
        world.snapshot
    );
    assert!(
        world.snapshot.contains("edited parked note body"),
        "the user's concurrent plain edit must survive in the commit:\n{}",
        world.snapshot
    );
    assert!(
        !world.snapshot.contains("<<<<<<<"),
        "the committed union must be conflict-free:\n{}",
        world.snapshot
    );
}

#[test]
fn finalize_carries_directive_edit_forward_instead_of_merging() {
    // #fintol3 (complement): a concurrent edit that adds a prompt/directive (a
    // `dispatch #…` scratch directive here) is NOT forward-merged — it stays on
    // today's fail-closed path so the directive is a next-cycle diff, not a
    // premature commit. The safety property the forward-merge must never weaken.
    let mut world = SimWorld::new(2_045);
    let baseline = concat!(
        "---\nsession: test\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "❯ Please reply\n",
        "<!-- /agent:exchange -->\n\n",
        "<!--\n-->\n",
    );
    let content_ours = baseline.replace(
        "<!-- /agent:exchange -->",
        "### Re: Please reply — gpt-5\n\nThe agent's answer body, long enough to matter.\n<!-- /agent:exchange -->",
    );
    // The user typed a scratch dispatch directive into the comment block.
    let candidate =
        content_ours.replace("<!--\n-->", "<!--\ndispatch #spec-test-build-install\n-->");

    world.finalize_ipc_candidate_with_tolerance(baseline, &content_ours, &candidate);

    assert_eq!(
        world.coverage.live_prompt_forward_merges, 0,
        "a directive-bearing edit must not forward-merge"
    );
    assert_eq!(
        world.coverage.ipc_snapshot_live_prompt_blocks, 1,
        "a directive-bearing edit keeps today's fail-closed carry-forward behavior"
    );
    assert_eq!(
        world.snapshot, content_ours,
        "the committed snapshot stays the agent response (the directive is not absorbed)"
    );
    assert!(
        world.doc.contains("dispatch #spec-test-build-install"),
        "the directive is carried forward in the live buffer for the next cycle:\n{}",
        world.doc
    );
}

// #smsim (document_cell_merge Phase 5): deterministic SimWorld coverage of the
// operator↔agent concurrent-edit matrix through the `#smconv` node-keyed
// convergence. Locks the merge/IPC data-loss family against regression:
// node-disjoint auto-merge, same-node operator-wins, operator-deleted-an-
// agent-edited-node ack, and turn-active-vs-unrelated-area ack gating.
#[test]
fn semmerge_sim_node_disjoint_operator_add_and_agent_strike_both_apply() {
    let mut world = SimWorld::new(2_051);
    let base = concat!(
        "<!-- agent:queue go -->\n",
        "- do [#a] task\n",
        "<!-- /agent:queue -->\n",
    );
    // Agent consumed + struck the head; operator concurrently added a new item.
    let agent_ours = concat!(
        "<!-- agent:queue go -->\n",
        "- ~~do [#a] task~~\n",
        "<!-- /agent:queue -->\n",
    );
    let operator_theirs = concat!(
        "<!-- agent:queue go -->\n",
        "- do [#a] task\n",
        "- do [#b] operator added\n",
        "<!-- /agent:queue -->\n",
    );

    let sm = world.converge_semantic_merge(base, agent_ours, operator_theirs, Some("queue"));

    assert!(
        sm.conflict_advisories.is_empty(),
        "disjoint merge needs no ack"
    );
    assert_eq!(world.coverage.document_cell_merge_node_disjoint, 1);
    assert!(
        world.snapshot.contains("~~do [#a] task~~"),
        "the agent strike must survive:\n{}",
        world.snapshot
    );
    assert!(
        world.snapshot.contains("do [#b] operator added"),
        "the concurrent operator add must survive:\n{}",
        world.snapshot
    );
}

#[test]
fn semmerge_sim_same_node_conflict_operator_wins_with_ack_in_active_area() {
    let mut world = SimWorld::new(2_052);
    let base = concat!(
        "<!-- agent:exchange -->\n",
        "- do [#a] original\n",
        "<!-- /agent:exchange -->\n",
    );
    let agent_ours = base.replace("original", "AGENT EDIT");
    let operator_theirs = base.replace("original", "OPERATOR EDIT");

    let sm = world.converge_semantic_merge(base, &agent_ours, &operator_theirs, Some("exchange"));

    assert_eq!(world.coverage.document_cell_merge_operator_wins, 1);
    assert_eq!(
        world.coverage.document_cell_merge_scope_gated_acks, 1,
        "an in-active-area same-node conflict must raise an ack"
    );
    assert!(
        !sm.conflict_advisories.is_empty(),
        "same-node conflict in the active area must ack"
    );
    let item_lines: Vec<&str> = world
        .snapshot
        .lines()
        .filter(|line| line.starts_with("- do [#a]"))
        .collect();
    assert_eq!(
        item_lines,
        vec!["- do [#a] OPERATOR EDIT"],
        "operator content must win the merged node:\n{}",
        world.snapshot
    );
    assert!(
        world.snapshot.contains("agent version not merged")
            && world.snapshot.contains("AGENT EDIT"),
        "the rejected agent side must be visible in the conflict note:\n{}",
        world.snapshot
    );
}

#[test]
fn semmerge_sim_same_node_conflict_outside_active_area_auto_resolves_no_ack() {
    let mut world = SimWorld::new(2_053);
    // Same same-node conflict, but the operator edited a node OUTSIDE the
    // turn-active area (queue, while only `exchange` is active). Operator still
    // wins the content, but no ack noise is raised (#smturnactive gating).
    let base = concat!(
        "<!-- agent:queue go -->\n",
        "- do [#a] original\n",
        "<!-- /agent:queue -->\n",
    );
    let agent_ours = base.replace("original", "AGENT EDIT");
    let operator_theirs = base.replace("original", "OPERATOR EDIT");

    let sm = world.converge_semantic_merge(base, &agent_ours, &operator_theirs, Some("exchange"));

    assert!(
        sm.conflict_advisories.is_empty(),
        "an out-of-active-area conflict must NOT raise ack noise: {:?}",
        sm.conflict_advisories
    );
    assert_eq!(world.coverage.document_cell_merge_scope_gated_acks, 0);
    assert!(
        world.snapshot.contains("OPERATOR EDIT"),
        "operator content still wins the merged doc:\n{}",
        world.snapshot
    );
}

#[test]
fn semmerge_sim_operator_deleted_agent_edited_node_keeps_deletion_and_acks() {
    let mut world = SimWorld::new(2_054);
    let base = concat!(
        "<!-- agent:exchange -->\n",
        "- do [#a] keep\n",
        "- do [#b] doomed\n",
        "<!-- /agent:exchange -->\n",
    );
    // Agent edited #b; operator concurrently DELETED #b entirely.
    let agent_ours = base.replace("doomed", "AGENT EDITED doomed");
    let operator_theirs = concat!(
        "<!-- agent:exchange -->\n",
        "- do [#a] keep\n",
        "<!-- /agent:exchange -->\n",
    );

    let sm = world.converge_semantic_merge(base, &agent_ours, operator_theirs, Some("exchange"));

    assert_eq!(
        world.coverage.document_cell_merge_delete_acks, 1,
        "operator-deleted-agent-edited node must raise a deletion ack"
    );
    assert!(
        sm.conflict_advisories.iter().any(|a| a.reason
            == agent_doc_merge::document_cell_merge::MergeConflictReason::OperatorDeletedAgentEditedNode),
        "ack reason must be operator-deleted-agent-edited-node"
    );
    assert!(
        !world.snapshot.contains("doomed"),
        "the operator deletion must stand (node not resurrected):\n{}",
        world.snapshot
    );
}

#[test]
fn hap7_sim_operator_queue_add_during_exchange_turn_no_duplication() {
    // #hap7 / #qdup (the operator-reported corruption shape): the agent writes a
    // new exchange `### Re:` turn while the operator concurrently inserts a queue
    // item. The scoped converge must land both with the operator's queue item
    // present EXACTLY ONCE and no adjacent queue node duplicated/reversed.
    let mut world = SimWorld::new(7_001);
    let base = concat!(
        "<!-- agent:queue -->\n",
        "- do [#a] first\n",
        "- do [#b] second\n",
        "<!-- /agent:queue -->\n",
        "<!-- agent:exchange -->\n",
        "### Re: prior — opus-4-8\n",
        "\n",
        "Prior answer.\n",
        "<!-- /agent:exchange -->\n",
    );
    // Agent (ours): appends a new exchange turn, queue untouched.
    let agent_ours = base.replace(
        "Prior answer.\n",
        "Prior answer.\n\n### Re: new turn — opus-4-8\n\nFresh response.\n",
    );
    // Operator (theirs): inserts a new queue item #c between #a and #b.
    let operator_theirs = base.replace(
        "- do [#a] first\n",
        "- do [#a] first\n- do [#c] full screen dialogs deploy\n",
    );

    let sm = world.converge_semantic_merge(base, &agent_ours, &operator_theirs, Some("exchange"));

    assert_eq!(
        world
            .snapshot
            .matches("do [#c] full screen dialogs deploy")
            .count(),
        1,
        "operator queue add must appear exactly once (no #qdup duplication):\n{}",
        world.snapshot
    );
    assert_eq!(
        world
            .snapshot
            .matches("### Re: new turn — opus-4-8")
            .count(),
        1,
        "agent's new exchange turn present exactly once:\n{}",
        world.snapshot
    );
    assert!(
        sm.conflict_advisories.is_empty(),
        "node-disjoint queue-add + exchange-turn need no ack: {:?}",
        sm.conflict_advisories
    );
}

#[test]
fn hap7_sim_operator_deleted_agent_targeted_node_noted_in_exchange() {
    // #hap7 / #qdup deleted-structure rule (end-to-end through the scoped converge):
    // the operator deletes a queue node the agent targeted this cycle. The deletion
    // stands (not resurrected) AND the fact is surfaced as a note inside
    // `agent:exchange` so the operator sees the dropped agent edit this cycle.
    let mut world = SimWorld::new(7_002);
    let base = concat!(
        "<!-- agent:queue -->\n",
        "- do [#a] first\n",
        "- do [#x] target node\n",
        "<!-- /agent:queue -->\n",
        "<!-- agent:exchange -->\n",
        "### Re: prior — opus-4-8\n",
        "\n",
        "Prior answer.\n",
        "<!-- /agent:exchange -->\n",
    );
    // Agent (ours): edited its targeted node #x.
    let agent_ours = base.replace(
        "- do [#x] target node\n",
        "- do [#x] target node AGENT EDITED\n",
    );
    // Operator (theirs): deleted #x entirely.
    let operator_theirs = base.replace("- do [#x] target node\n", "");

    let sm = world.converge_semantic_merge(base, &agent_ours, &operator_theirs, Some("exchange"));

    // Deletion stands — #x not resurrected, agent's edit not merged back.
    assert!(
        !world.snapshot.contains("target node AGENT EDITED"),
        "agent's edit to the deleted node must NOT be merged back:\n{}",
        world.snapshot
    );
    // The fact is surfaced both as a structured note and inside agent:exchange.
    assert!(
        sm.exchange_notes.iter().any(|n| n.contains("#x")),
        "a deletion note naming #x must be surfaced: {:?}",
        sm.exchange_notes
    );
    assert!(
        world.snapshot.contains("operator deleted") && world.snapshot.contains("#x"),
        "the deletion note must land in the converged document:\n{}",
        world.snapshot
    );
}

// #samplepcdrift2: the recurring `ipc_socket_already_applied_live_buffer_diverged`
// drift must always be RECOVERED, never silently lost. When the socket reports
// `already_applied` but the live buffer diverged with the assistant response
// fragmented out of `exchange` (plus a fresh user keystroke), the recovery
// materializes the response back from `content_ours` so the committed snapshot
// carries the full response and the live user edit survives. The target is zero
// UNRECOVERED drift — clean detect+recover is acceptable, silent loss is not.
#[test]
fn already_applied_diverged_sim_recovers_dropped_response_without_loss() {
    let mut world = SimWorld::new(2_037);
    world.append_to_exchange("❯ Please reply\n").unwrap();
    let baseline = world.doc.clone();
    let expected_response = "### Re: Please reply — gpt-5\n\nAnswered.";
    let content_ours = baseline.replace(
        "<!-- /agent:exchange -->",
        "### Re: Please reply — gpt-5\n\nAnswered.\n<!-- /agent:exchange -->",
    );
    // The already_applied socket ack came back, but the live buffer diverged:
    // the response fragmented out of exchange (only the prompt survives) and the
    // operator typed the next prompt mid-finalize. The recovery must not lose
    // the response.
    world.doc = baseline.replace(
        "<!-- /agent:exchange -->",
        "❯ typing the next prompt mid-finalize\n<!-- /agent:exchange -->",
    );

    world.recover_already_applied_diverged_response(&content_ours, expected_response);

    assert_eq!(
        world.coverage.already_applied_response_recoveries, 1,
        "the dropped response must trigger exactly one recovery"
    );
    assert_eq!(
        world.doc.matches("### Re: Please reply — gpt-5").count(),
        1,
        "the dropped response must be materialized back exactly once (zero UNRECOVERED drift)"
    );
    assert!(
        world.doc.contains("❯ typing the next prompt mid-finalize"),
        "the live user keystroke must be preserved alongside the recovered response"
    );
    assert_eq!(
        world.snapshot, content_ours,
        "the committed snapshot adopts content_ours (baseline + full response)"
    );

    // Idempotent: re-running recovery once the response is materialized is a
    // no-op — no duplicate heading, no extra recovery count.
    world.recover_already_applied_diverged_response(&content_ours, expected_response);
    assert_eq!(
        world.coverage.already_applied_response_recoveries, 1,
        "recovery must not re-fire once the response is already materialized"
    );
    assert_eq!(
        world.doc.matches("### Re: Please reply — gpt-5").count(),
        1,
        "no duplicate response heading after idempotent recovery"
    );
}

#[test]
fn recovery_projection_normalization_divergence_sim_uses_normalized_content_ours() {
    let mut world = SimWorld::new(2_015);
    let recovery_projection = template_doc(
        "do #sidecardiv. spec-test-build-install-commit-push\n### Re: #sidecardiv — gpt-5\n\nDone.\n",
    );
    let content_ours = recovery_projection.clone();
    let normalize_prefix_lines =
        vec!["do #sidecardiv. spec-test-build-install-commit-push".to_string()];

    world.apply_canonical_normalization_recovery(
        &recovery_projection,
        &content_ours,
        &normalize_prefix_lines,
    );

    assert_eq!(
        world.coverage.recovery_projection_normalization_divergences,
        1
    );
    assert!(
        world
            .snapshot
            .contains("❯ do #sidecardiv. spec-test-build-install-commit-push"),
        "rejected recovery projection should fall back to normalized content_ours"
    );
    assert_eq!(
        world
            .snapshot
            .matches("### Re: #sidecardiv — gpt-5")
            .count(),
        1,
        "normalization fallback must not duplicate the assistant response"
    );
}

#[test]
fn ack_projection_only_drift_sim_blocks_without_visible_proof() {
    let mut world = SimWorld::new(2_012);
    world.doc = template_doc(
        "❯ do #acksidecar. spec-test-build-install-commit-push\n<!-- agent:boundary:live -->\n",
    );
    let original_snapshot = template_doc("<!-- agent:boundary:base -->\n");
    world.snapshot = original_snapshot.clone();
    let ack_content = template_doc(
        "❯ do #acksidecar. spec-test-build-install-commit-push\n### Re: #acksidecar — gpt-5\n\nDone.\n<!-- agent:boundary:ack -->\n",
    );

    world.handle_ack_projection_only_evidence(&ack_content);

    assert_eq!(world.coverage.ack_projection_only_repairs, 0);
    assert_eq!(world.coverage.ack_projection_only_blocks, 1);
    assert_eq!(
        world.snapshot, original_snapshot,
        "projection-only ACK evidence must not become the committed snapshot when the visible file still lags"
    );
    assert!(
        !world.doc.contains("### Re: #acksidecar — gpt-5"),
        "the sim must keep projection-only evidence distinct from ordinary disk repair"
    );
}

#[test]
fn ack_projection_only_matching_visible_sim_can_refresh_snapshot() {
    let mut world = SimWorld::new(2_014);
    let ack_content = template_doc(
        "❯ do #ackvisible. spec-test-build-install-commit-push\n### Re: #ackvisible — gpt-5\n\nDone.\n<!-- agent:boundary:ack -->\n",
    );
    world.doc = ack_content.clone();
    world.snapshot = template_doc("<!-- agent:boundary:base -->\n");

    world.handle_ack_projection_only_evidence(&ack_content);

    assert_eq!(world.coverage.ack_projection_only_repairs, 1);
    assert_eq!(world.coverage.ack_projection_only_blocks, 0);
    assert_eq!(
        world.snapshot, ack_content,
        "ACK-content can refresh durable state only when it matches the operator-visible document"
    );
}

#[test]
fn visible_duplicate_repair_sim_dedupes_real_response_block() {
    let mut world = SimWorld::new(2_013);
    world.doc = template_doc(
        "❯ do #dupvis. spec-test-build-install-commit-push\n### Re: #dupvis — gpt-5\n\nDone.\n### Re: #dupvis — gpt-5\n\nDone.\n<!-- agent:boundary:dup -->\n",
    );

    world.repair_visible_duplicate_response();

    assert_eq!(world.coverage.visible_duplicate_repairs, 1);
    assert_eq!(
        world.doc.matches("### Re: #dupvis — gpt-5").count(),
        1,
        "visible duplicate repair should remove the real repeated response block"
    );
    world.assert_structural_invariants().unwrap();
}

#[test]
fn normalization_repair_sim_uses_narrow_patch_for_prefix_only_divergence() {
    let mut world = SimWorld::new(2_011);
    world.doc = template_doc(
        "do #simnorm. spec-test-build-install-commit-push\n### Re: #simnorm — gpt-5\n\nDone.\n",
    );

    world.apply_narrow_normalization_repair(&[
        "do #simnorm. spec-test-build-install-commit-push".to_string()
    ]);

    assert_eq!(world.coverage.normalization_repair_patches, 1);
    assert!(
        world
            .doc
            .contains("❯ do #simnorm. spec-test-build-install-commit-push"),
        "narrow normalization repair should prefix the live prompt"
    );
    assert_eq!(
        world.doc.matches("### Re: #simnorm — gpt-5").count(),
        1,
        "narrow repair must not duplicate the assistant response"
    );
}

#[test]
fn post_commit_follow_up_sim_keeps_prompt_out_of_snapshot() {
    let mut world = SimWorld::new(2_014);
    world.apply(SimCommand::EditPrompt).unwrap();
    world.apply(SimCommand::CaptureResponse).unwrap();
    world.apply(SimCommand::ApplyCapturedResponse).unwrap();
    world.apply(SimCommand::Commit).unwrap();
    let committed_snapshot = world.snapshot.clone();

    world.apply(SimCommand::EditLaterPrompt).unwrap();
    world.record_post_commit_follow_up_handoff().unwrap();

    assert_eq!(world.phase, CyclePhase::Committed);
    assert_eq!(world.coverage.post_commit_follow_up_handoffs, 1);
    assert_eq!(
        world.snapshot, committed_snapshot,
        "terminal follow-up drift should not be absorbed into the committed snapshot"
    );
    assert!(
        world.doc.contains("later follow-up"),
        "the user follow-up stays visible for the next response cycle"
    );
}

#[test]
fn route_sim_recovers_busy_interrupt_to_ready_once() {
    let mut world = SimWorld::new(2_005);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorWaitingInput).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(world.coverage.route_dispatch_acceptances, 0);

    world.apply(SimCommand::BusyInterruptRecoveryReady).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    world.apply(SimCommand::ProveDispatchAccepted).unwrap();

    assert_eq!(world.coverage.busy_interrupt_recoveries, 1);
    assert_eq!(world.coverage.route_dispatch_acceptances, 1);
    assert_eq!(world.coverage.route_dispatch_proofs, 1);
}

#[test]
fn route_sim_rejects_stale_generation_and_pane_observations() {
    let mut world = SimWorld::new(2_002);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    world.apply(SimCommand::StaleSupervisorUpdate).unwrap();
    assert_eq!(world.coverage.stale_generation_blocks, 1);

    world.apply(SimCommand::ObserveStalePane).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(world.coverage.stale_pane_blocks, 1);

    world.apply(SimCommand::RepairProjection).unwrap();
    world.apply(SimCommand::ObserveMissingPane).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(world.coverage.missing_pane_blocks, 1);
}

#[test]
fn route_sim_repairs_projection_drift_from_durable_actor_state() {
    let mut world = SimWorld::new(2_003);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    world.apply(SimCommand::DriftProjection).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(world.coverage.projection_drift_blocks, 1);
    assert_eq!(world.coverage.route_dispatch_acceptances, 0);

    world.apply(SimCommand::RepairProjection).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    world.apply(SimCommand::ProveDispatchAccepted).unwrap();
    assert_eq!(world.coverage.projection_repairs, 1);
    assert_eq!(world.coverage.route_dispatch_acceptances, 1);
    assert_eq!(world.coverage.route_dispatch_proofs, 1);
}

#[test]
fn sync_sim_tmuxbudget_seed_3001_attaches_requested_pane_around_protected_cycle() {
    let mut world = SimWorld::new(3_001);
    world.apply(SimCommand::SyncProtectedGrowthManual).unwrap();

    assert_eq!(
        world.sync.visible,
        vec![
            "protected".to_string(),
            "sibling".to_string(),
            "requested".to_string()
        ],
        "manual sync should attach the requested pane around a protected closeout owner instead of deferring"
    );
    assert_eq!(world.coverage.sync_protected_expansions, 1);
    assert_eq!(world.coverage.sync_focus_handoffs, 1);

    world.apply(SimCommand::SyncProtectedGrowthPassive).unwrap();
    assert_eq!(
        world.sync.visible,
        vec![
            "protected".to_string(),
            "sibling".to_string(),
            "requested".to_string()
        ],
        "safe-passive sync should share the same protected-closeout attach decision"
    );
    assert_eq!(world.coverage.sync_protected_expansions, 2);
    assert_eq!(world.coverage.sync_focus_handoffs, 2);
}

#[test]
fn sync_sim_tmuxbudget_seed_3002_replaces_detachable_pane_while_protected_cycle_remains() {
    let mut world = SimWorld::new(3_002);
    world
        .apply(SimCommand::SyncDetachableReplaceManual)
        .unwrap();

    assert_eq!(
        world.sync.visible,
        vec!["protected".to_string(), "requested".to_string()],
        "manual sync should displace an unprotected unwanted pane instead of treating any protected visible pane as a no-op"
    );
    assert_eq!(world.sync.active.as_deref(), Some("requested"));
    assert_eq!(world.coverage.sync_detachable_replacements, 1);

    world
        .apply(SimCommand::SyncDetachableReplacePassive)
        .unwrap();
    assert_eq!(
        world.sync.visible,
        vec!["protected".to_string(), "requested".to_string()],
        "safe-passive sync should share the same detachable-pane replacement decision"
    );
    assert_eq!(world.coverage.sync_detachable_replacements, 2);
    assert_eq!(world.coverage.sync_focus_handoffs, 2);
}

#[test]
fn sync_sim_tmuxbudget_seed_3003_preserve_layout_still_reselects_visible_focus() {
    let mut world = SimWorld::new(3_003);
    world.apply(SimCommand::SyncVisibleFocusPreserve).unwrap();

    assert_eq!(
        world.sync.visible,
        vec!["protected".to_string(), "sibling".to_string()],
        "preserve-layout sync should not mutate the visible pane set"
    );
    assert_eq!(world.sync.active.as_deref(), Some("sibling"));
    assert_eq!(world.coverage.sync_preserve_layout_blocks, 1);
    assert_eq!(world.coverage.sync_focus_handoffs, 1);
}

#[test]
fn sync_sim_tmuxbudget_seed_3005_rejects_duplicate_editor_pane_for_rerequested_document() {
    // Regression guard for "3 tmux panes with 2 editor panes": when the editor
    // document is already visible and sync re-requests the same document (the
    // duplicate-claim / pane-id churn surface), the projection must keep a single
    // editor pane instead of attaching a second one.
    let mut world = SimWorld::new(3_005);
    world
        .apply(SimCommand::SyncRerequestVisibleEditorManual)
        .unwrap();
    assert_eq!(
        world.sync.visible,
        vec!["editor".to_string()],
        "manual sync must not attach a second editor pane when the document is already visible"
    );
    assert_eq!(world.sync.active.as_deref(), Some("editor"));
    // The structural cardinality invariant must hold: no duplicate visible pane.
    world.assert_structural_invariants().unwrap();

    world
        .apply(SimCommand::SyncRerequestVisibleEditorPassive)
        .unwrap();
    assert_eq!(
        world.sync.visible,
        vec!["editor".to_string()],
        "safe-passive sync must share the same single-editor-pane decision"
    );
    world.assert_structural_invariants().unwrap();
}

#[test]
fn sync_sim_repeated_resync_preserves_exact_editor_cardinality_and_stash_parking() {
    // Regression for the Haiven 2 → 5 → 3 pane oscillation. Registry repair
    // preserves live registered actors in stash; one reactive exact-visible
    // generation owns the visible projection. Repeating the same operator
    // resync must therefore remain idempotent at exactly two visible panes.
    let mut world = SimWorld::new(3_008);
    world.sync = SyncProjection::reactive_resync_case();
    let expected_visible = world.sync.visible.clone();
    let expected_stashed = world.sync.stashed.clone();

    for _ in 0..4 {
        world
            .apply(SimCommand::SyncReactiveResyncGeneration)
            .unwrap();
        assert_eq!(
            world.sync.visible, expected_visible,
            "resync must not promote unrelated registered stash panes"
        );
        assert_eq!(
            world.sync.stashed, expected_stashed,
            "resync must preserve reactive parking for non-visible documents"
        );
        assert_eq!(
            world.sync.visible.len(),
            2,
            "two editor panes must produce exactly two tmux panes"
        );
        world.assert_structural_invariants().unwrap();
    }
}

#[test]
fn sync_sim_exact_visible_focus_swaps_offscreen_owned_pane_into_view() {
    // `#exact-visible-focus-swap`: a deliberate editor tab/focus change emits
    // `sync --focus <file> --exact-visible --no-autostart`. Regression: focusing a
    // document whose owned pane was alive but off-screen left the previously-visible
    // document's pane in place — exact-visible sync blocked every file and preserved
    // the stale layout instead of reaching the reconcile. With a proven-live owner,
    // the off-screen pane must be swapped into view.
    let mut world = SimWorld::new(3_006);
    world
        .apply(SimCommand::SyncExactVisibleFocusResolvesOffscreenOwner)
        .unwrap();
    assert_eq!(
        world.sync.visible,
        vec!["focused".to_string()],
        "exact-visible focus must swap the off-screen owned pane into the visible layout"
    );
    assert!(
        world.sync.stashed.contains("onscreen"),
        "the previously-visible document's pane must be stashed, not left on screen"
    );
    assert_eq!(
        world.sync.active.as_deref(),
        Some("focused"),
        "the focused document's pane must become the active pane"
    );
    assert_eq!(world.coverage.sync_detachable_replacements, 1);
    assert_eq!(world.coverage.sync_focus_handoffs, 1);
    // `active` must never point at a stashed pane.
    world.assert_structural_invariants().unwrap();
}

#[test]
fn sync_sim_exact_visible_focus_preserves_layout_when_owner_unproven() {
    // `#exact-visible-focus-swap` safe fallback: when the off-screen pane cannot
    // prove ownership (e.g. the supervisor identity is unavailable mid-recycle),
    // exact-visible sync must preserve the current layout instead of borrowing or
    // swapping a pane — never cold-start or steal a pane on an unproven owner.
    let mut world = SimWorld::new(3_007);
    world
        .apply(SimCommand::SyncExactVisibleFocusUnprovenPreserve)
        .unwrap();
    assert_eq!(
        world.sync.visible,
        vec!["onscreen".to_string()],
        "unproven-owner exact-visible focus must preserve the visible layout"
    );
    assert!(
        world.sync.stashed.contains("focused"),
        "the unproven off-screen pane must stay stashed"
    );
    assert_eq!(world.coverage.sync_preserve_layout_blocks, 1);
    world.assert_structural_invariants().unwrap();
}

#[test]
fn sync_sim_tmuxbudget_seed_3004_attaches_hidden_requested_pane_and_focuses_visible_sibling() {
    let mut world = SimWorld::new(3_004);
    world
        .apply(SimCommand::SyncProtectedGrowthFocusVisible)
        .unwrap();

    assert_eq!(
        world.sync.visible,
        vec![
            "protected".to_string(),
            "sibling".to_string(),
            "requested".to_string()
        ],
        "safe-passive sync should attach the hidden requested pane without detaching the protected closeout owner"
    );
    assert_eq!(
        world.sync.active.as_deref(),
        Some("sibling"),
        "safe-passive sync should still focus an already-visible requested sibling"
    );
    assert_eq!(world.coverage.sync_protected_expansions, 1);
    assert_eq!(world.coverage.sync_focus_handoffs, 1);
}

#[test]
fn finalize_with_typing_in_post_exchange_comment_and_already_applied_receipt_does_not_duplicate_response()
 {
    // Plan: tasks/agent-doc/plan-ipc-corruption-and-duplicate-during-typing.md
    // Phase 1 (deterministic SimWorld repro) + Phase 5 (regression coverage).
    //
    // Models the finalize-time IPC corruption + duplicate-response race:
    //   1. Document baseline = exchange with a prompt + an HTML scratch comment
    //      below `</agent:exchange>` that the user is actively typing into.
    //   2. Agent runs finalize and the plugin had already applied the response
    //      patch to its live buffer (e.g. via a prior socket retry whose receipt
    //      write was slow).
    //   3. The plugin's retry receipt is the protocol's dedupe signal:
    //      `{"type":"receipt","status":"applied","reason":"already_applied"}`.
    //   4. The binary recognizes that signal through
    //      `agent_doc_ipc_protocol::is_already_applied_receipt_error_message`
    //      and skips the file-IPC
    //      fallback so it does not re-apply the same response on top of the
    //      live buffer (which would land a duplicate `### Re:` heading and
    //      collide with the user's in-flight typing inside the scratch
    //      comment).
    let prompt = "do #ipcdup. spec-test-build-install-commit-push";
    let file = Path::new("sim-ipc-already-applied.md");
    let mut world = SimWorld::new(2_020);
    world.append_to_exchange(&format!("❯ {prompt}\n")).unwrap();
    world
        .insert_after_exchange(&post_exchange_scratch_comment(prompt))
        .unwrap();
    world.snapshot = world.doc.clone();

    let response_block = "\n### Re: closeout — gpt-5\n\nImplemented.\n";
    world
        .append_to_exchange(response_block)
        .expect("plugin already inserted the response patch into the live buffer");
    let live_after_plugin_apply = world.doc.clone();

    let already_applied_receipt =
        r#"{"type":"receipt","status":"applied","reason":"already_applied"}"#;
    assert_eq!(
        agent_doc_ipc_protocol::classify_socket_receipt(already_applied_receipt),
        agent_doc_ipc_protocol::SocketReceiptClassification::AlreadyApplied,
        "protocol contract: receipt reason=already_applied is the dedupe signal"
    );
    let send_err = format!("IPC receipt already_applied: {already_applied_receipt}");
    assert!(
        agent_doc_ipc_protocol::is_already_applied_receipt_error_message(&send_err),
        "send_message wraps already_applied receipts in an error the write path can recognize"
    );

    assert_eq!(
        world.doc.matches("### Re: closeout — gpt-5").count(),
        1,
        "with the already_applied gate, the file-IPC fallback must not re-apply the patch and double the response heading"
    );
    assert_owned_scratch_comment_preserved(&world.doc, prompt);

    let mut counterfactual = world.doc.clone();
    counterfactual.push_str(response_block);
    assert_eq!(
        counterfactual.matches("### Re: closeout — gpt-5").count(),
        2,
        "without the already_applied gate the file-IPC fallback would land a duplicate response heading"
    );
    let (deduped, changed) = agent_doc_write_converge_io::dedupe_ipc_snapshot_content(
        file,
        Some(&live_after_plugin_apply),
        &counterfactual,
        "sim_ipc_already_applied_counterfactual",
    )
    .unwrap();
    assert!(
        changed,
        "dedupe recovery must collapse the counterfactual duplicate response"
    );
    assert_eq!(
        deduped.matches("### Re: closeout — gpt-5").count(),
        1,
        "dedupe recovery must collapse the duplicated response heading down to one"
    );
    assert_owned_scratch_comment_preserved(&deduped, prompt);
}

#[test]
fn cycle_1779845677327_scratch_directives_survive_already_applied_ipc_race() {
    let prompt = "The duplicate content corrupting document and duplicate prompt issues happened yet again. Reproduce bugs with tests first that fail and fix the implementation.";
    let scratch_directive = "#spec-test-build-install-commit-push";
    let scratch_dispatch = "dispatch #spec-test-build-install-commit-push";
    let scratch_comment =
        format!("\n###\n\n<!--\n{prompt}\n{scratch_directive}\n---\n{scratch_dispatch}\n-->\n");
    let file = Path::new("cycle-1779845677327.md");
    let mut world = SimWorld::new(1_779_845_677_327);
    world
        .append_to_exchange(&format!("❯ do [#liveipcrace]\n❯ {scratch_directive}\n"))
        .unwrap();
    world.insert_after_exchange(&scratch_comment).unwrap();
    world.snapshot = world.doc.clone();

    let response_block = "\n### Re: cycle 1779845677327 IPC race — gpt-5\n\nImplemented.\n";
    world
        .append_to_exchange(response_block)
        .expect("plugin already inserted the response patch into the live buffer");
    let live_after_plugin_apply = world.doc.clone();

    let already_applied_receipt =
        r#"{"type":"receipt","status":"applied","reason":"already_applied"}"#;
    assert_eq!(
        agent_doc_ipc_protocol::classify_socket_receipt(already_applied_receipt),
        agent_doc_ipc_protocol::SocketReceiptClassification::AlreadyApplied,
        "editor plugins must use already_applied receipts so the binary skips file IPC fallback"
    );

    assert_eq!(
        world
            .doc
            .matches("### Re: cycle 1779845677327 IPC race — gpt-5")
            .count(),
        1,
        "already_applied must leave one response heading"
    );
    assert_eq!(
        world.doc.matches(prompt).count(),
        1,
        "scratch prompt text should remain visible exactly once"
    );
    assert!(
        world.doc.contains(&scratch_comment),
        "scratch prompt preset and dispatch directives must remain inside the ordinary comment:\n{}",
        world.doc
    );

    let mut counterfactual = world.doc.clone();
    counterfactual.push_str(response_block);
    let (deduped, changed) = agent_doc_write_converge_io::dedupe_ipc_snapshot_content(
        file,
        Some(&live_after_plugin_apply),
        &counterfactual,
        "cycle_1779845677327_counterfactual",
    )
    .unwrap();
    assert!(
        changed,
        "dedupe recovery must collapse the counterfactual duplicate response"
    );
    assert_eq!(
        deduped
            .matches("### Re: cycle 1779845677327 IPC race — gpt-5")
            .count(),
        1,
        "dedupe recovery must collapse the duplicated response heading down to one"
    );
    assert!(
        deduped.contains(&scratch_comment),
        "dedupe recovery must not discard prompt preset/directive text in the scratch comment:\n{deduped}"
    );
}

#[test]
fn ipc_snapshot_guard_blocks_live_queue_drift_after_preflight() {
    let mut world = SimWorld::new(2_022);
    world
        .insert_after_exchange("\n\n## Queue\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n")
        .unwrap();
    let baseline = world.doc.clone();
    let response = response_patch("live queue IPC race");
    let (patches, unmatched) = agent_doc_template::parse_patches(&response).unwrap();
    let content_ours =
        agent_doc_template_io::apply_patches(&baseline, &patches, &unmatched, Path::new("sim.md"))
            .unwrap();
    let live_queue_prompt = "- do #liveipcrace. #spec-test-build-install-commit-push";
    let ack_candidate = content_ours.replace(
        "<!-- agent:queue -->\n<!-- /agent:queue -->",
        &format!("<!-- agent:queue -->\n{live_queue_prompt}\n<!-- /agent:queue -->"),
    );

    assert!(
        agent_doc_document_realtime::write_policy::ipc_snapshot_would_absorb_live_prompt_drift_after_preflight(
            &baseline,
            &ack_candidate,
            &content_ours,
        ),
        "IPC snapshot adoption must classify live queue edits typed after preflight as next-cycle drift"
    );

    world.doc = ack_candidate;
    world.snapshot = content_ours;
    assert!(
        world.doc.contains(live_queue_prompt),
        "live queue prompt should remain visible in the working tree"
    );
    assert!(
        !world.snapshot.contains(live_queue_prompt),
        "snapshot should stay on content_ours so the queue prompt remains a future diff"
    );
}

#[test]
fn closeout_recovery_transition_scenarios_cover_simworld_inputs() {
    use agent_doc_document_realtime::write_policy::FullContentVisibleReplacementDecision;
    use agent_doc_turn::closeout_recovery::{
        CloseoutRecoveryDecision, CloseoutRecoveryDecisionInput, CloseoutRecoveryState,
        closeout_recovery_decision_from_state,
    };

    let file = Path::new("sim.md");
    let recovery_command = Some("agent-doc recover sim.md");

    // Queue edits around capture/write fragmentation stay visible, but do not get
    // adopted into the committed snapshot.
    let mut queue_world = SimWorld::new(2_260);
    queue_world
        .insert_after_exchange("\n\n## Queue\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n")
        .unwrap();
    let baseline = queue_world.doc.clone();
    let response = response_patch("transition queue drift");
    let (patches, unmatched) = agent_doc_template::parse_patches(&response).unwrap();
    let content_ours =
        agent_doc_template_io::apply_patches(&baseline, &patches, &unmatched, file).unwrap();
    let live_queue_prompt = "- do #transitionqueue. spec-test-build-install-commit-push";
    let queue_candidate = content_ours.replace(
        "<!-- agent:queue -->\n<!-- /agent:queue -->",
        &format!("<!-- agent:queue -->\n{live_queue_prompt}\n<!-- /agent:queue -->"),
    );
    queue_world.adopt_ipc_snapshot_candidate(&baseline, &content_ours, &queue_candidate);
    assert_eq!(queue_world.coverage.ipc_snapshot_live_prompt_blocks, 1);
    assert!(queue_world.doc.contains(live_queue_prompt));
    assert!(!queue_world.snapshot.contains(live_queue_prompt));
    assert_eq!(
        closeout_recovery_decision_from_state(
            CloseoutRecoveryState::UnsafeUserContentDrift,
            CloseoutRecoveryDecisionInput::default(),
            recovery_command,
        )
        .as_str(),
        "blocked"
    );

    // Compaction/full-content replacement must reject stale source buffers after
    // live typing, which maps to an open closeout block unless route/JB prompt
    // context is available to queue behind it.
    let mut compact_world = SimWorld::new(2_261);
    assert_eq!(
        compact_world
            .stale_full_content_visible_replacement(FullContentReplacementSource::CompactExchange),
        FullContentVisibleReplacementDecision::RejectStaleSourceBuffer
    );
    assert_eq!(compact_world.coverage.stale_source_buffer_skips, 1);
    match closeout_recovery_decision_from_state(
        CloseoutRecoveryState::OpenCycle,
        CloseoutRecoveryDecisionInput::default(),
        recovery_command,
    ) {
        CloseoutRecoveryDecision::Blocked { missing_proof, .. } => {
            assert!(missing_proof.contains("open cycle"), "{missing_proof}");
        }
        other => panic!("open closeout without prompt context must block: {other:?}"),
    }
    assert_eq!(
        closeout_recovery_decision_from_state(
            CloseoutRecoveryState::OpenCycle,
            CloseoutRecoveryDecisionInput {
                prompt_context_available: true,
                blocker_reason: Some("JB Run Agent Doc during open closeout"),
                stale_capture_supersession_proof: None,
            },
            recovery_command,
        ),
        CloseoutRecoveryDecision::QueuePromptForAfterCloseout {
            state: CloseoutRecoveryState::OpenCycle,
            reason: "JB Run Agent Doc during open closeout".to_string(),
        }
    );

    // Stale/projection-only ACK evidence is kept distinct from visible-file repair,
    // and superseded captures retire only with proof.
    let mut ack_world = SimWorld::new(2_262);
    ack_world.doc = template_doc(
        "❯ do #transitionack. spec-test-build-install-commit-push\n<!-- agent:boundary:live -->\n",
    );
    ack_world.snapshot = template_doc("<!-- agent:boundary:base -->\n");
    let ack_content = template_doc(
        "❯ do #transitionack. spec-test-build-install-commit-push\n### Re: #transitionack — gpt-5\n\nDone.\n<!-- agent:boundary:ack -->\n",
    );
    ack_world.handle_ack_projection_only_evidence(&ack_content);
    assert_eq!(ack_world.coverage.ack_projection_only_repairs, 0);
    assert_eq!(ack_world.coverage.ack_projection_only_blocks, 1);
    assert!(
        !ack_world
            .snapshot
            .contains("### Re: #transitionack — gpt-5")
    );
    assert_eq!(
        closeout_recovery_decision_from_state(
            CloseoutRecoveryState::RecoveryProjectionVisibleDrift,
            CloseoutRecoveryDecisionInput::default(),
            recovery_command,
        )
        .as_str(),
        "refresh_recovery_projections_from_visible"
    );
    assert_eq!(
        closeout_recovery_decision_from_state(
            CloseoutRecoveryState::MissingResponseBody,
            CloseoutRecoveryDecisionInput {
                stale_capture_supersession_proof: Some("visible response superseded stale ACK"),
                ..CloseoutRecoveryDecisionInput::default()
            },
            recovery_command,
        ),
        CloseoutRecoveryDecision::RetireStaleCapture {
            state: CloseoutRecoveryState::MissingResponseBody,
            proof: "visible response superseded stale ACK".to_string(),
        }
    );

    // The already_applied ACK path recovers dropped response content without
    // duplicating it; this is the stale-ACK counterexample for unsafe replay.
    let mut already_world = SimWorld::new(2_263);
    already_world
        .append_to_exchange("❯ Please reply\n")
        .unwrap();
    let baseline = already_world.doc.clone();
    let content_ours = baseline.replace(
        "<!-- /agent:exchange -->",
        "### Re: Please reply — gpt-5\n\nAnswered.\n<!-- /agent:exchange -->",
    );
    already_world.doc = baseline.replace(
        "<!-- /agent:exchange -->",
        "❯ next prompt while ACK is stale\n<!-- /agent:exchange -->",
    );
    already_world.recover_already_applied_diverged_response(
        &content_ours,
        "### Re: Please reply — gpt-5\n\nAnswered.",
    );
    assert_eq!(
        already_world.coverage.already_applied_response_recoveries,
        1
    );
    assert_eq!(
        already_world
            .doc
            .matches("### Re: Please reply — gpt-5")
            .count(),
        1
    );
    assert!(
        already_world
            .doc
            .contains("❯ next prompt while ACK is stale")
    );
}

// -------- #queue-strike-on-halt: a halt/refusal response must not strike the
// active auto-queue head; only an explicit closeout flag advances it. The Codex
// Stop-hook heading path is exact-match only. See
// tasks/agent-doc/plan-queue-strike-on-halt-response.md.

#[test]
fn halt_response_does_not_strike_queue_head_but_done_flag_does() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "queue_active: true\n",
        "---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue auto -->\n",
        "- do [#alpha]\n",
        "- do [#beta]\n",
        "<!-- /agent:queue -->\n",
    );
    std::fs::write(&doc, content).unwrap();
    agent_doc_snapshot_io::checkpoint_document_baseline(
        &doc,
        content,
        agent_doc_ops_log_io::log_op,
    )
    .unwrap();

    // A halt response that names the head with a trailing modifier must NOT
    // register as targeting the head (exact-topic match only).
    let active_head = agent_doc_queue::queue_heads::active_queue_head_text(content)
        .unwrap()
        .unwrap();
    let halt = "### Re: do [#alpha] halt — opus-4-8\n\nBacklog left intact; not executing.\n";
    assert!(
        !agent_doc_queue::queue_response::response_explicitly_targets_queue_head(
            halt,
            &active_head,
        ),
        "halt heading must not target the queue head"
    );
    // An exact-topic heading still registers, preserving the Codex auto-loop on a
    // clean completion that titles the response with the head prompt verbatim.
    let exact = "### Re: do [#alpha] — opus-4-8\n\nDone.\n";
    assert!(
        agent_doc_queue::queue_response::response_explicitly_targets_queue_head(
            exact,
            &active_head,
        ),
        "exact-topic heading should still target the queue head"
    );

    // An explicit --done strikes the head, leaving #beta as the next head.
    let outcome =
        agent_doc_queue_io::queue_consume::consume_queue_prompts_for_done_ids_force_disk_with_outcome(
            &doc,
            &["alpha".to_string()],
            &crate::CLI_QUEUE_CONSUME_WRITE_EFFECTS,
        )
        .unwrap()
        .expect("explicit --done should consume the queue head");
    assert_eq!(outcome.consumed_count, 1);
    assert_eq!(outcome.remaining, 1);
    let after = std::fs::read_to_string(&doc).unwrap();
    assert!(
        after.contains("- ~~do [#alpha]~~"),
        "alpha struck:\n{after}"
    );
    assert!(
        after.contains("- do [#beta]"),
        "beta remains the head:\n{after}"
    );
}

// -------- #adoc-bdauc-simworld: deterministic SimWorld coverage for baseline
// drift after a manual user commit. See
// tasks/agent-doc/plan-baseline-drift-after-user-commit.md.

#[test]
fn baseline_drift_benign_user_commit_outside_response_auto_refreshes() {
    let response = response_patch("baseline drift");
    let (dir, doc, capture, mut world) = setup_baseline_drift_capture(20260525 + 10, &response);
    apply_response_and_save_current(&doc, &mut world, &response).unwrap();

    world
        .replace_component_content(
            "backlog",
            "- [ ] [#tigersim] Implement the simulator MVP\n- [ ] [#manual] User-added follow-up outside the captured response\n",
        )
        .unwrap();
    std::fs::write(&doc, &world.doc).unwrap();
    agent_doc_snapshot_io::checkpoint_document_baseline(
        &doc,
        &world.doc,
        agent_doc_ops_log_io::log_op,
    )
    .unwrap();

    agent_doc_capture_io::validate_replay(&doc, &capture)
        .expect("benign user commit outside response must auto-refresh");

    let refreshed = agent_doc_capture_io::load_active(&doc).unwrap().unwrap();
    assert_eq!(
        refreshed.file_hash.as_deref(),
        Some(agent_doc_hash::content_hash(&world.doc).as_str()),
        "file hash should refresh to the user-committed document"
    );
    assert_eq!(
        refreshed.snapshot_hash.as_deref(),
        Some(agent_doc_hash::content_hash(&world.doc).as_str()),
        "the ledger may identify the logical baseline without making its cold projection live authority"
    );
    let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        ops_log.contains("capture_baseline_refreshed_for_benign_drift"),
        "benign drift refresh should be auditable:\n{ops_log}"
    );
}

#[test]
fn baseline_drift_user_edit_inside_committed_response_fails_closed() {
    let response = response_patch("baseline drift");
    let (_dir, doc, capture, mut world) = setup_baseline_drift_capture(20260525 + 11, &response);
    apply_response_and_save_current(&doc, &mut world, &response).unwrap();

    world.doc = world.doc.replace(
        "Implemented and verified.",
        "User rewrote the committed response.",
    );
    std::fs::write(&doc, &world.doc).unwrap();
    agent_doc_snapshot_io::checkpoint_document_baseline(
        &doc,
        &world.doc,
        agent_doc_ops_log_io::log_op,
    )
    .unwrap();

    let err = agent_doc_capture_io::validate_replay(&doc, &capture)
        .expect_err("editing the committed response body must fail closed");
    assert!(
        err.to_string().contains("baseline no longer matches")
            || err.to_string().contains("snapshot no longer matches"),
        "expected fail-closed baseline drift error; got: {err}"
    );
}

#[test]
fn baseline_drift_user_edit_matches_normalized_response_adopts() {
    let response = "<!-- patch:exchange -->\n### Re: baseline drift normalized — gpt-5\n\nImplemented and verified.\n❯ Submodule pointer updated.\n<!-- /patch:exchange -->\n";
    let (_dir, doc, capture, mut world) = setup_baseline_drift_capture(20260525 + 12, response);
    apply_response_and_save_current(&doc, &mut world, response).unwrap();

    world.doc = world
        .doc
        .replace("❯ Submodule pointer updated.", "Submodule pointer updated.");
    std::fs::write(&doc, &world.doc).unwrap();
    agent_doc_snapshot_io::checkpoint_document_baseline(
        &doc,
        &world.doc,
        agent_doc_ops_log_io::log_op,
    )
    .unwrap();

    agent_doc_capture_io::validate_replay(&doc, &capture)
        .expect("user-normalized response body should be adopted");

    let refreshed = agent_doc_capture_io::load_active(&doc).unwrap().unwrap();
    assert_eq!(
        refreshed.file_hash.as_deref(),
        Some(agent_doc_hash::content_hash(&world.doc).as_str()),
        "file hash should reflect the normalized user-cleaned response"
    );
    assert_eq!(
        refreshed.snapshot_hash.as_deref(),
        Some(agent_doc_hash::content_hash(&world.doc).as_str()),
        "the ledger may identify the logical baseline without making its cold projection live authority"
    );
}

// -------- #jbccc1: deterministic SimWorld scenario for the JB File Cache Conflict
// cancel wedge. See tasks/agent-doc/plan-jb-cache-cancel-stuck-cycle.md.
//
// The scenario models three branches of the JetBrains "File Cache Conflict"
// dialog that surfaces during IPC patch delivery while the document is open
// in IntelliJ:
//
// 1. **No dialog** — happy path; IPC apply commits cleanly.
// 2. **Accept** — user accepts the conflict resolution; IPC apply still commits
//    cleanly. From the binary's perspective indistinguishable from (1).
// 3. **Cancel** — user cancels; the response is already written to the working
//    tree by the time the dialog appears, but the binary's IPC callback returns
//    failure. The cycle wedges at `WriteApplied`: response in doc, snapshot ≠
//    HEAD-equivalent, no commit landed.
//
// The cancel branch is the failing baseline that Phase 3 (#jbccc3) will recover
// from inside `preflight` / `repair` by auto-detecting the
// `cycle_phase=WriteApplied + working-tree-response-matches-captured-body +
// snapshot != HEAD` signature and running the equivalent of `write --commit`.

/// Apply the captured response to the document the way a successful JB cache
/// conflict acceptance would: VFS writes the patch into the working tree and
/// the binary receives the ack. From the binary's perspective this is
/// indistinguishable from `SimCommand::ApplyCapturedResponse`.
fn apply_jb_cache_conflict_accept(world: &mut SimWorld) {
    world.apply(SimCommand::ApplyCapturedResponse).unwrap();
}

/// Apply the captured response to the document the way a JB cache conflict
/// **cancel** does: the VFS write has already happened (the plugin queued it
/// before showing the dialog), but the binary's IPC callback returns failure
/// and the cycle never reaches `Committed`. The world stops at `WriteApplied`
/// with the response visible in `self.doc` and the snapshot still pointing at
/// the state before response capture.
///
/// This is the root-cause hypothesis 1/2 from
/// `tasks/agent-doc/plan-jb-cache-cancel-stuck-cycle.md`.
fn apply_jb_cache_conflict_cancel(world: &mut SimWorld) {
    world.apply(SimCommand::ApplyCapturedResponse).unwrap();
    // No SimCommand::Commit — the cancel branch is exactly the absence of the
    // commit half. Phase 3 (#jbccc3) will close this in the binary.
}

#[test]
fn jb_cache_conflict_no_dialog_commits_cleanly() {
    let mut world = SimWorld::new(20260525);
    world.apply(SimCommand::EditPrompt).unwrap();
    world.apply(SimCommand::CaptureResponse).unwrap();
    world.apply(SimCommand::ApplyCapturedResponse).unwrap();
    world.apply(SimCommand::Commit).unwrap();
    assert_eq!(world.phase, CyclePhase::Committed);
    assert_eq!(world.snapshot, world.doc);
    world.strict_closeout_invariants().unwrap();
}

#[test]
fn jb_cache_conflict_accept_branch_commits_cleanly() {
    let mut world = SimWorld::new(20260525 + 1);
    world.apply(SimCommand::EditPrompt).unwrap();
    world.apply(SimCommand::CaptureResponse).unwrap();
    apply_jb_cache_conflict_accept(&mut world);
    world.apply(SimCommand::Commit).unwrap();
    assert_eq!(world.phase, CyclePhase::Committed);
    assert_eq!(world.snapshot, world.doc);
    world.strict_closeout_invariants().unwrap();
}

#[test]
fn jb_cache_conflict_cancel_branch_wedges_at_write_applied_today() {
    // FAILING BASELINE for #jbccc1.
    //
    // Today the cancel branch leaves the cycle at WriteApplied with the
    // response visible in the document but snapshot ≠ doc. This test pins
    // that behavior so that #jbccc3 (binary auto-recovery) has a clear
    // before/after assertion to flip.
    //
    // When #jbccc3 lands, this test should be replaced (or its assertion
    // inverted) by `jb_cache_conflict_cancel_branch_auto_recovers_via_preflight`
    // that drives a recovery and asserts phase == Committed without a manual
    // `write --commit`.
    let mut world = SimWorld::new(20260525 + 2);
    world.apply(SimCommand::EditPrompt).unwrap();
    world.apply(SimCommand::CaptureResponse).unwrap();
    apply_jb_cache_conflict_cancel(&mut world);

    assert_eq!(
        world.phase,
        CyclePhase::WriteApplied,
        "cancel branch must wedge at WriteApplied today"
    );
    assert_ne!(
        world.snapshot, world.doc,
        "cancel branch must leave snapshot behind the visible response today"
    );
    assert!(
        world.doc.contains("### Re: sim closeout"),
        "cancel branch must leave the response visible in the working-tree doc today"
    );

    let err = world
        .strict_closeout_invariants()
        .expect_err("cancel-branch wedge must trip strict closeout today");
    assert!(
        err.to_string()
            .contains("response write applied but not committed"),
        "expected wedge to surface as 'response write applied but not committed'; got: {err}"
    );
}

#[test]
fn jb_cache_conflict_cancel_branch_recovers_via_explicit_write_commit_today() {
    // Today's documented recovery path (per the #adoc-jb-cache-cancel-stuck-cycle
    // backlog item) is: run `agent-doc write --commit <FILE>` manually after the
    // cancel-induced wedge. In the simulator, that's a follow-up `Commit` from
    // the WriteApplied phase. This test pins the recovery exit so #jbccc3 can
    // automate it inside preflight without regressing the manual escape hatch.
    let mut world = SimWorld::new(20260525 + 3);
    world.apply(SimCommand::EditPrompt).unwrap();
    world.apply(SimCommand::CaptureResponse).unwrap();
    apply_jb_cache_conflict_cancel(&mut world);
    assert_eq!(world.phase, CyclePhase::WriteApplied);

    // Manual recovery — equivalent to `agent-doc write --commit <FILE>`.
    world.apply(SimCommand::Commit).unwrap();
    assert_eq!(world.phase, CyclePhase::Committed);
    assert_eq!(world.snapshot, world.doc);
    world.strict_closeout_invariants().unwrap();
}

/// Phase 5 regression (#jbccc5): after the Phase 3 (#jbccc3) binary fix lands,
/// the next `agent-doc preflight` automatically runs `git::commit` for the
/// cancel-induced wedge — no manual `agent-doc write --commit` required. The
/// simulator mirrors that auto-recovery as a follow-up `Commit` driven by the
/// preflight contract rather than the operator. This test pins the
/// post-Phase-3 invariant: from the WriteApplied wedge, the binary-owned
/// recovery alone is sufficient to reach `Committed` with `strict_closeout_invariants`
/// clean.
#[test]
fn jb_cache_conflict_cancel_branch_auto_recovers_via_preflight() {
    let mut world = SimWorld::new(20260525 + 4);
    world.apply(SimCommand::EditPrompt).unwrap();
    world.apply(SimCommand::CaptureResponse).unwrap();
    apply_jb_cache_conflict_cancel(&mut world);
    assert_eq!(world.phase, CyclePhase::WriteApplied);
    assert_ne!(world.snapshot, world.doc);

    // Auto-recovery — driven by the next `agent-doc preflight`, not by the
    // operator. In real code this is preflight detecting the cancel pattern
    // (cycle phase WriteApplied + snapshot ≠ HEAD + working tree matches
    // snapshot modulo transient markers) and dispatching `git::commit`.
    world.apply(SimCommand::Commit).unwrap();

    assert_eq!(world.phase, CyclePhase::Committed);
    assert_eq!(world.snapshot, world.doc);
    world.strict_closeout_invariants().unwrap();
}

// -------- #jbccacceptdup: deterministic SimWorld scenario for the JB File
// Cache Conflict **late-accept replay** wedge.
//
// See tasks/agent-doc/plan-jb-cache-conflict-accept-duplicates-response.md and
// the repro logged on tasks/agent-doc/agent-doc-bugs2.md [#wkbs].
//
// Real-world incidents repro'd this on 2026-05-26 (`tasks/software/tsift.md`)
// and 2026-05-27 (`sample-app/tasks/astro-listings.md`). Sequence:
//
// 1. The agent-doc cycle reaches `Committed` cleanly (response in HEAD).
// 2. While the cycle was open, IntelliJ surfaced a File Cache Conflict dialog
//    that the user did not resolve. The plugin stashed the IPC patch as a
//    conflict-deferred payload instead of mutating the open document.
// 3. Hours later the user accepts the dialog. The plugin replays the deferred
//    payload against the open document. Because the response body is already
//    in the committed file, the replay appends a second `### Re: …` block
//    to the working tree.
// 4. The next `agent-doc preflight` reaches the drift-recovery branch and
//    auto-commits the duplicated state — the dupe wedges into HEAD.
//
// Today the closeout invariants only check `has_duplicate_response_heading()`
// at phase `WriteApplied`, so once a duplicate lands *after* `Committed`, the
// existing strict invariant is silent. This scenario pins the failing baseline
// (duplicate visible in working tree post-commit) and the documented manual
// recovery (`dedupe_responses` + re-commit) so a future binary-side guard has
// a clear before/after assertion to flip.

#[test]
fn jb_cache_conflict_accept_late_replays_duplicate_response_today() {
    // FAILING BASELINE for #jbccacceptdup.
    //
    // After a committed cycle, the late-accepted conflict replays a stale
    // response payload back into the working tree. `self.doc` ends up with two
    // `### Re: sim closeout` blocks while `self.snapshot` still matches the
    // single-response commit — exactly the shape observed on
    // `sample-app/tasks/astro-listings.md` on 2026-05-27.
    //
    // When the binary-side fix lands (plan steps 2–5 in
    // `plan-jb-cache-conflict-accept-duplicates-response.md`), the plugin /
    // IPC apply path will revalidate against HEAD before mutation and skip
    // the replay. This test should then be replaced (or its assertions
    // inverted) by `jb_cache_conflict_accept_late_replay_rejected_at_apply`.
    let mut world = SimWorld::new(20260527);
    world.apply(SimCommand::EditPrompt).unwrap();
    world.apply(SimCommand::CaptureResponse).unwrap();
    apply_jb_cache_conflict_accept(&mut world);
    world.apply(SimCommand::Commit).unwrap();
    assert_eq!(world.phase, CyclePhase::Committed);
    assert_eq!(world.snapshot, world.doc);

    let committed_snapshot = world.snapshot.clone();

    // Hours later: the long-pending File Cache Conflict dialog is accepted and
    // the plugin replays its deferred IPC payload on top of the already-
    // committed response.
    world.apply(SimCommand::DuplicateVisibleResponse).unwrap();

    assert_eq!(
        world.doc.matches("### Re: sim closeout").count(),
        2,
        "late accept must replay the stale response into the working tree"
    );
    assert_eq!(
        world.snapshot, committed_snapshot,
        "snapshot must still match the original committed cycle until the dupe is recommitted"
    );
    assert!(
        world.has_duplicate_response_heading(),
        "duplicate-response detector must observe the replayed block"
    );
}

#[test]
fn jb_cache_conflict_accept_late_replay_manual_repair_recovers_today() {
    // Today's documented manual recovery path for #jbccacceptdup: the operator
    // resolves the duplicate by hand (the replayed block often has a different
    // body than the committed one, so `dedupe_responses` — which only collapses
    // identical-body duplicates — can't always help) and then crosses the
    // binary-owned commit boundary via `agent-doc write --commit <FILE>` or
    // `agent-doc commit <FILE>`. In the simulator that's an explicit `world.doc`
    // edit (modeling the operator's edit) followed by a re-commit so the
    // snapshot catches up. This test pins the recovery exit so a future
    // binary-side replay guard can automate it without regressing the manual
    // escape hatch.
    let mut world = SimWorld::new(20260527 + 1);
    world.apply(SimCommand::EditPrompt).unwrap();
    world.apply(SimCommand::CaptureResponse).unwrap();
    apply_jb_cache_conflict_accept(&mut world);
    world.apply(SimCommand::Commit).unwrap();
    let committed_doc = world.doc.clone();
    world.apply(SimCommand::DuplicateVisibleResponse).unwrap();
    assert_eq!(world.doc.matches("### Re: sim closeout").count(), 2);

    // Manual recovery: operator deletes the replayed block, restoring the
    // committed doc. Then a re-commit updates the snapshot.
    world.doc = committed_doc;
    world.snapshot = world.doc.clone();

    assert_eq!(
        world.doc.matches("### Re: sim closeout").count(),
        1,
        "manual repair must leave a single response block in the working tree"
    );
    assert_eq!(world.snapshot, world.doc);
    world.strict_closeout_invariants().unwrap();
}

// ============================================================================
// #swint — SimWorld editor + tmux integration harness
//
// `SimEditor` is a deterministic in-harness actor that speaks the same durable
// Lazily/CRDT protocol as the JetBrains / VS Code plugins: it publishes the
// editor-visible buffer through the canonical replica relay and reads "current document" back
// through the *production* `realtime_model::resolve_current_doc` seam (rung 3b,
// `#rtwatch`). This lets a SimWorld scenario exercise the editor-buffer-vs-disk
// read-authority reconcile, multi-editor CRDT relay, and the
// tmux dispatch/drain integrated system (`#kp5z`) *without a live IDE* — turning
// the File-Cache-Conflict / IPC-drift / queue-flood live-verify-only classes
// into deterministic regressions.
//
// See tasks/agent-doc/plan-simworld-editor-integration.md.
// ============================================================================

use agent_doc_document_realtime::{BufferState, DocAuthority, Reconciliation};

/// Which editor's live-buffer protocol a [`SimEditor`] emulates. The read
/// authority contract is identical across kinds (a dirty buffer is always
/// authoritative over stale disk); the kind only changes how an *external* disk
/// write that lands while the buffer is dirty is surfaced to the user — the
/// File-Cache-Conflict semantics that actually bite (Slice 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditorKind {
    /// No editor-specific conflict modeling — the pure realtime-model seam.
    Generic,
    /// IntelliJ / JetBrains: a read-only VFS WatchService notices the external
    /// disk change and raises a modal "File Cache Conflict" dialog. The buffer
    /// stays authoritative until the user resolves the dialog.
    JetBrains,
    /// VS Code: buffer events keep the dirty buffer; an external disk change is
    /// flagged non-modally and the in-memory buffer stays authoritative.
    VsCode,
    /// Zed: the production extension is currently staged for editor-authority
    /// support, but the deterministic peer uses the same non-modal dirty-buffer
    /// authority contract so three-replica schedules can be exercised now.
    Zed,
}

impl EditorKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Generic => "generic",
            Self::JetBrains => "jetbrains",
            Self::VsCode => "vscode",
            Self::Zed => "zed",
        }
    }
}

/// How a [`SimEditor`] of a given [`EditorKind`] reconciles an external disk
/// write (e.g. agent-doc's own patchback) that lands while the buffer holds
/// unsaved edits. Both editors keep the buffer authoritative (no clobber) until
/// the user resolves; only the surfaced signal differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CacheConflict {
    /// Buffer was clean (in sync with disk): the external write is adopted
    /// silently — there is nothing to conflict with.
    NoneAdopted,
    /// JetBrains modal File-Cache-Conflict dialog raised; buffer stays dirty.
    JetBrainsDialog,
    /// VS Code non-modal "file changed on disk" badge; buffer stays dirty.
    VsCodeKeepBuffer,
    /// Zed keeps the dirty buffer and surfaces the external change without
    /// replacing the editor-owned text.
    ZedKeepBuffer,
}

/// A deterministic editor-buffer actor that speaks the CRDT relay protocol
/// against a real on-disk document, so SimWorld scenarios can drive the
/// production read-authority reconcile without a live IDE.
struct SimEditor {
    kind: EditorKind,
    path: PathBuf,
    liveness_tag: String,
    replica_identity: String,
    replica: agent_doc_merge::crdt_sync::ReplicaState,
    buffer: String,
    dirty: bool,
    generation: u64,
}

impl SimEditor {
    /// Attach an editor of `kind` to an existing on-disk document. The buffer
    /// starts equal to disk (clean, in sync).
    fn attach(kind: EditorKind, path: &Path) -> Result<Self> {
        Self::attach_with_id(kind, path, kind.as_str())
    }

    fn attach_with_id(kind: EditorKind, path: &Path, editor_id: &str) -> Result<Self> {
        let _disk_buffer = std::fs::read_to_string(path)
            .map_err(|err| anyhow!("SimEditor attach read {}: {err}", path.display()))?;
        let key = editor_buffer_key(path);
        let document_hash = agent_doc_hash::document_id_for_path(path);
        let liveness_tag = format!("sim-editor:{editor_id}:{key}");
        agent_doc_reliable_sync_io::global_liveness_plane()
            .lock()
            .restore_liveness(&[agent_doc_reliable_sync_io::liveness::LivenessOp::Open {
                document_hash,
                pid: std::process::id().into(),
                tag: liveness_tag.clone(),
            }]);
        let replica_identity = format!("{editor_id}:{key}");
        let (client_id, bootstrap) =
            agent_doc_crdt_relay_io::register_replica_for_file(path, &replica_identity)?
                .ok_or_else(|| anyhow!("SimEditor attach could not register CRDT replica"))?;
        let replica =
            agent_doc_merge::crdt_sync::ReplicaState::from_encoded(client_id, &bootstrap)?;
        let buffer = replica.text();
        Ok(Self {
            kind,
            path: path.to_path_buf(),
            liveness_tag,
            replica_identity,
            replica,
            buffer,
            dirty: false,
            generation: 0,
        })
    }

    fn generic(path: &Path) -> Result<Self> {
        Self::attach(EditorKind::Generic, path)
    }

    fn jetbrains(path: &Path) -> Result<Self> {
        Self::attach(EditorKind::JetBrains, path)
    }

    fn vscode(path: &Path) -> Result<Self> {
        Self::attach(EditorKind::VsCode, path)
    }

    fn zed(path: &Path) -> Result<Self> {
        Self::attach(EditorKind::Zed, path)
    }

    /// Type an unsaved edit: the buffer now holds `content` ahead of disk. Publishes
    /// the local edit through the CRDT relay so the production realtime feed
    /// surfaces it as a genuine unsaved edit (the `#queue-user-edit-overwrite`
    /// no-clobber hazard this whole plan exists to defend).
    fn type_unsaved(&mut self, content: &str) -> Result<()> {
        self.publish_buffer_replace(content)?;
        self.dirty = true;
        self.generation += 1;
        Ok(())
    }

    /// Apply one editor-local delta without first pulling peer updates. Keeping
    /// this operation separate from delivery lets tests model genuinely
    /// concurrent edits from several editor replicas against the same frontier.
    fn type_unsaved_delta(&mut self, offset: usize, delete_len: usize, insert: &str) -> Result<()> {
        let end = offset
            .checked_add(delete_len)
            .filter(|end| *end <= self.buffer.len())
            .ok_or_else(|| anyhow!("SimEditor local delta is outside the buffer"))?;
        if !self.buffer.is_char_boundary(offset) || !self.buffer.is_char_boundary(end) {
            return Err(anyhow!("SimEditor local delta splits a UTF-8 code point"));
        }
        self.replica
            .apply_local_edit(offset as u32, delete_len as u32, insert);
        let update = self.replica.encode_state();
        agent_doc_crdt_relay_io::relay_replica_update_for_file(
            &self.path,
            &self.replica_identity,
            &update,
        )?
        .ok_or_else(|| anyhow!("SimEditor relay update refused under detached authority"))?;
        self.buffer.replace_range(offset..end, insert);
        self.dirty = true;
        self.generation += 1;
        Ok(())
    }

    /// Pull every pending peer delivery through the same full-state projection contract the
    /// production plugins use. Returns the number of remote deliveries applied.
    fn pull_peer_updates(&mut self) -> Result<usize> {
        let Some(pull) = agent_doc_crdt_relay_io::pull_replica_updates_for_file(
            &self.path,
            &self.replica_identity,
        )?
        else {
            return Ok(0);
        };
        let mut applied = 0;
        for update in pull.updates {
            self.replica.apply_update(&update.update)?;
            applied += 1;
        }
        if applied > 0 {
            self.buffer = self.replica.text();
            let projected = agent_doc_crdt_relay_io::observe_replica_projection_for_file(
                &self.path,
                &self.replica_identity,
                &agent_doc_hash::content_hash(&self.buffer),
            )?
            .ok_or_else(|| anyhow!("SimEditor projection refused under detached authority"))?;
            if !projected {
                return Err(anyhow!(
                    "SimEditor relay did not project its coalesced visible state"
                ));
            }
            self.dirty = true;
            self.generation += 1;
        }
        Ok(applied)
    }

    /// Flush the buffer to disk (Ctrl-S): buffer == disk, clean. The relay
    /// remains the current-document authority; disk becomes its saved projection.
    fn save(&mut self) -> Result<()> {
        std::fs::write(&self.path, &self.buffer)
            .map_err(|err| anyhow!("SimEditor save write {}: {err}", self.path.display()))?;
        self.dirty = false;
        self.generation += 1;
        Ok(())
    }

    /// Close the document in the editor through replica deregistration plus the
    /// reliable-sync Lazily OR-set close event.
    fn close(self) -> Result<()> {
        let _ = agent_doc_crdt_relay_io::deregister_replica_for_file(
            &self.path,
            &self.replica_identity,
        )?;
        let document_hash = agent_doc_hash::document_id_for_path(&self.path);
        agent_doc_reliable_sync_io::global_liveness_plane()
            .lock()
            .restore_liveness(&[agent_doc_reliable_sync_io::liveness::LivenessOp::Close {
                document_hash,
                pid: std::process::id().into(),
                observed_tags: vec![self.liveness_tag.clone()],
            }]);
        Ok(())
    }

    /// Reload from disk after the controller wrote+committed the document model
    /// (Slice 4 broadcast-back): buffer == disk, clean.
    fn reload_from_disk(&mut self) -> Result<()> {
        let disk = std::fs::read_to_string(&self.path)
            .map_err(|err| anyhow!("SimEditor reload read {}: {err}", self.path.display()))?;
        self.publish_buffer_replace(&disk)?;
        self.dirty = false;
        self.generation += 1;
        Ok(())
    }

    /// Model an external disk write (agent-doc patchback) landing while the editor
    /// is open. Returns the kind-specific [`CacheConflict`] surfaced to the user.
    /// A clean buffer silently adopts the new disk content; a dirty buffer ahead
    /// of disk stays authoritative (no clobber) and the editor re-reports its
    /// still-unsaved buffer so the realtime feed keeps preferring it.
    fn external_disk_write(&mut self, content: &str) -> Result<CacheConflict> {
        std::fs::write(&self.path, content).map_err(|err| {
            anyhow!(
                "SimEditor external_disk_write {}: {err}",
                self.path.display()
            )
        })?;
        if !self.dirty {
            self.publish_buffer_replace(content)?;
            self.generation += 1;
            return Ok(CacheConflict::NoneAdopted);
        }
        // The canonical relay still contains the dirty editor cut, so the
        // external disk projection cannot outrank it.
        self.generation += 1;
        Ok(match self.kind {
            EditorKind::JetBrains => CacheConflict::JetBrainsDialog,
            EditorKind::Zed => CacheConflict::ZedKeepBuffer,
            // VS Code and the generic seam both keep the dirty buffer non-modally.
            EditorKind::VsCode | EditorKind::Generic => CacheConflict::VsCodeKeepBuffer,
        })
    }

    /// Resolve "current document" through the *production* realtime model
    /// (`try_resolve_current_doc_from_file`), the exact seam `preflight` / `write` /
    /// `session-check` source the current doc through.
    fn resolve(&self) -> Result<Reconciliation> {
        agent_doc_document_realtime_io::try_resolve_current_doc_from_file(&self.path)
    }

    fn publish_buffer_replace(&mut self, content: &str) -> Result<()> {
        let delete_len = self.buffer.len() as u32;
        self.replica.apply_local_edit(0, delete_len, content);
        let update = self.replica.encode_state();
        agent_doc_crdt_relay_io::relay_replica_update_for_file(
            &self.path,
            &self.replica_identity,
            &update,
        )?
        .ok_or_else(|| anyhow!("SimEditor relay update refused under detached authority"))?;
        self.buffer = content.to_string();
        Ok(())
    }

    /// The pure [`BufferState`] this editor currently holds — what the plugin
    /// reports over IPC. Feeds the seam-isolated `reconcile_current_doc` primitive
    /// directly (vs the durable-feed `resolve`, which suppresses an in-sync buffer
    /// to `None` and so reports `editor_absent` rather than `in_sync`).
    fn buffer_state(&self) -> BufferState {
        BufferState::new(self.buffer.clone(), self.dirty, self.generation)
    }
}

/// Canonical Lazily editor-replica key for a document path: canonicalize,
/// falling back to the raw path.
fn editor_buffer_key(path: &Path) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string()
}

/// A template document with one exchange prompt — the on-disk baseline a
/// `SimEditor` attaches to.
fn editor_baseline_doc() -> String {
    template_doc("❯ do #sim-existing. spec-test-build-install-commit-push\n")
}

/// [`editor_baseline_doc`] with an extra well-formed backlog line spliced in —
/// the shape of a queue/backlog item a user types in the editor without saving.
fn editor_doc_with_backlog(extra_line: &str) -> String {
    editor_baseline_doc().replace(
        "- [ ] [#tigersim] Implement the simulator MVP\n",
        &format!("- [ ] [#tigersim] Implement the simulator MVP\n{extra_line}"),
    )
}

/// Build a temp project with `.agent-doc/{snapshots,logs}` and the document on
/// disk. Returns the live `TempDir` (keep it in scope) and the document path.
fn editor_project(disk: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
    // SimEditor is an in-process deterministic editor fixture. The production
    // path observes CP-owned relay state; seed the in-process Lazily model
    // directly so there is no filesystem state to drift.
    let doc = dir.path().join("doc.md");
    std::fs::write(&doc, disk).unwrap();
    agent_doc_crdt_relay_io::seed_embedded_relay_for_file(&doc).unwrap();
    (dir, doc)
}

/// Drive a finalize cycle on `baseline` (the resolved current document): capture
/// the agent response, apply it, and commit. Returns the committed `SimWorld` so
/// callers can assert the committed snapshot.
fn finalize_on_resolved(seed: u64, baseline: &str) -> SimWorld {
    let mut world = SimWorld::new(seed);
    world.doc = baseline.to_string();
    world.snapshot = baseline.to_string();
    world.apply(SimCommand::CaptureResponse).unwrap();
    world.apply(SimCommand::ApplyCapturedResponse).unwrap();
    world.apply(SimCommand::Commit).unwrap();
    world
}

// -------- Slice 1: SimEditor buffer actor (foundation) --------

#[test]
fn simeditor_unsaved_buffer_edit_resolves_to_editor_buffer_and_survives_commit() {
    // #swint Slice 1 acceptance — the deterministic form of the live-only
    // `#rtwverify` proof: an unsaved buffer edit is authoritative over stale disk
    // and survives the agent's commit, with the grep-able ops.log marker.
    let disk = editor_baseline_doc();
    let buffer = editor_doc_with_backlog(
        "- [ ] [#buffer-only-edit] user typed this in IDEA without saving\n",
    );
    let (dir, doc) = editor_project(&disk);

    let mut editor = SimEditor::generic(&doc).unwrap();
    // Clean open buffer still resolves through the CRDT relay: the editor buffer
    // is authoritative while the editor is attached, even when it matches disk.
    assert_eq!(
        editor.resolve().unwrap().authority,
        DocAuthority::EditorBuffer
    );

    editor.type_unsaved(&buffer).unwrap();
    let reconciliation = editor.resolve().unwrap();
    assert_eq!(
        reconciliation.authority,
        DocAuthority::EditorBuffer,
        "the production realtime model must read the unsaved buffer, not stale disk"
    );
    assert!(
        reconciliation.content.contains("#buffer-only-edit"),
        "resolve must surface the unsaved buffer edit"
    );
    assert_eq!(reconciliation.content, buffer);

    let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        ops_log.contains("realtime_doc_resolve authority=editor_buffer"),
        "ops.log must record the editor-buffer authority decision:\n{ops_log}"
    );

    // The agent's response commits on top of the buffer the cycle read, so the
    // unsaved edit crosses the commit boundary instead of being clobbered.
    let world = finalize_on_resolved(20260611, &reconciliation.content);
    assert_eq!(world.phase, CyclePhase::Committed);
    assert!(
        world.snapshot.contains("#buffer-only-edit"),
        "the unsaved buffer edit must survive the agent's commit (no clobber)"
    );
    assert!(
        world.snapshot.contains("### Re: sim closeout"),
        "the agent response committed on top of the buffer content"
    );
    world.strict_closeout_invariants().unwrap();
    drop(dir);
}

#[test]
fn simeditor_save_then_close_falls_back_to_disk_authority() {
    // The "falling back to the file on disk" half of the authority model: saving
    // does not demote a live editor buffer; closing the editor does.
    let disk = editor_baseline_doc();
    let buffer = editor_doc_with_backlog("- [ ] [#unsaved-then-saved] typed then saved\n");
    let (dir, doc) = editor_project(&disk);

    let mut editor = SimEditor::generic(&doc).unwrap();
    editor.type_unsaved(&buffer).unwrap();
    assert_eq!(
        editor.resolve().unwrap().authority,
        DocAuthority::EditorBuffer
    );

    editor.save().unwrap();
    let disk_now = std::fs::read_to_string(&doc).unwrap();
    assert!(
        disk_now.contains("#unsaved-then-saved"),
        "save flushed to disk"
    );
    // Pure seam: a present, in-sync buffer is disk-canonical with reason `in_sync`.
    let in_sync =
        agent_doc_document_realtime::reconcile_current_doc(&disk_now, Some(&editor.buffer_state()));
    assert_eq!(in_sync.authority, DocAuthority::Disk);
    assert_eq!(in_sync.reason, "in_sync");
    // Durable seam: the live CRDT relay remains authoritative while attached,
    // even when the buffer text matches disk.
    assert_eq!(
        editor.resolve().unwrap().authority,
        DocAuthority::EditorBuffer
    );

    editor.close().unwrap();
    let closed = agent_doc_document_realtime_io::try_resolve_current_doc_from_file(&doc).unwrap();
    assert_eq!(closed.authority, DocAuthority::Disk);
    assert_eq!(
        closed.reason, "editor_absent",
        "closing the document must deregister the Lazily editor replica"
    );
    drop(dir);
}

// -------- Slice 2: JB + VS Code protocol parity fixtures --------

#[test]
fn simeditor_jb_and_vscode_buffer_authority_parity_with_kind_specific_conflict() {
    // #swint Slice 2 / File-Cache-Conflict class (#w42v): JetBrains and VS Code
    // must agree on read authority (a dirty buffer always wins) while differing
    // only on the surfaced cache-conflict signal for an external disk write.
    for kind in [EditorKind::JetBrains, EditorKind::VsCode] {
        let disk = editor_baseline_doc();
        let marker = format!("#{}-unsaved", kind.as_str());
        let buffer = editor_doc_with_backlog(&format!(
            "- [ ] [{marker}] typed in {} without saving\n",
            kind.as_str()
        ));
        let (dir, doc) = editor_project(&disk);
        let mut editor = SimEditor::attach(kind, &doc).unwrap();

        // Parity 1: a clean open buffer resolves through the CRDT relay
        // regardless of kind.
        assert_eq!(
            editor.resolve().unwrap().authority,
            DocAuthority::EditorBuffer,
            "{kind:?}: clean open buffer stays editor-authoritative"
        );

        // Parity 2: an unsaved edit is authoritative regardless of kind.
        editor.type_unsaved(&buffer).unwrap();
        let r = editor.resolve().unwrap();
        assert_eq!(
            r.authority,
            DocAuthority::EditorBuffer,
            "{kind:?}: dirty buffer must win over disk"
        );
        assert!(r.content.contains(&marker));

        // Divergence: the cache-conflict signal for an external disk write while
        // the buffer is dirty is kind-specific (modal dialog vs non-modal badge).
        let conflict = editor.external_disk_write(&disk).unwrap();
        let expected = match kind {
            EditorKind::JetBrains => CacheConflict::JetBrainsDialog,
            EditorKind::VsCode => CacheConflict::VsCodeKeepBuffer,
            EditorKind::Generic | EditorKind::Zed => {
                unreachable!("loop only covers JB + VS Code")
            }
        };
        assert_eq!(conflict, expected, "{kind:?}: cache-conflict signal");

        // Parity 3: despite the external write, the unsaved buffer is still
        // authoritative — the conflict never silently clobbers the user's edit.
        let after = editor.resolve().unwrap();
        assert_eq!(
            after.authority,
            DocAuthority::EditorBuffer,
            "{kind:?}: external write must not clobber the unsaved buffer"
        );
        assert!(after.content.contains(&marker));

        // A clean buffer, by contrast, silently adopts the external write while
        // remaining editor-authoritative until it closes.
        editor.save().unwrap();
        assert_eq!(
            editor.external_disk_write(&disk).unwrap(),
            CacheConflict::NoneAdopted,
            "{kind:?}: a clean buffer adopts the external write with no conflict"
        );
        assert_eq!(
            editor.resolve().unwrap().authority,
            DocAuthority::EditorBuffer
        );
        drop(dir);
    }
}

#[test]
fn three_simulated_editors_are_equal_peers_and_reconnect_to_the_same_crdt_cut() {
    // Cross-editor SimWorld: JetBrains, VS Code, and Zed each own an independent
    // replica. All three edit from the same frontier before receiving peer
    // deliveries, then converge through the production relay + ACK path. Zed is
    // intentionally included here even while its real plugin parity row remains
    // staged: simulation proves the protocol contract without claiming the
    // production extension has shipped editor authority.
    let disk = "shared baseline\n";
    let (dir, doc) = editor_project(disk);
    let mut jetbrains = SimEditor::jetbrains(&doc).unwrap();
    let mut vscode = SimEditor::vscode(&doc).unwrap();
    let mut zed = SimEditor::zed(&doc).unwrap();
    let frontier = disk.len();

    jetbrains
        .type_unsaved_delta(frontier, 0, "jetbrains peer\n")
        .unwrap();
    vscode
        .type_unsaved_delta(frontier, 0, "vscode peer\n")
        .unwrap();
    zed.type_unsaved_delta(frontier, 0, "zed peer\n").unwrap();

    for _ in 0..3 {
        jetbrains.pull_peer_updates().unwrap();
        vscode.pull_peer_updates().unwrap();
        zed.pull_peer_updates().unwrap();
    }

    let converged = jetbrains.buffer.clone();
    assert_eq!(vscode.buffer, converged);
    assert_eq!(zed.buffer, converged);
    for marker in ["jetbrains peer", "vscode peer", "zed peer"] {
        assert!(
            converged.contains(marker),
            "three-peer convergence lost {marker}: {converged:?}"
        );
    }

    // A disconnected peer must not block the live pair. When it reconnects, its
    // controller bootstrap catches it up to the exact same canonical cut.
    zed.close().unwrap();
    let next_offset = jetbrains.buffer.len();
    jetbrains
        .type_unsaved_delta(next_offset, 0, "after zed disconnect\n")
        .unwrap();
    vscode.pull_peer_updates().unwrap();
    let reconnected_zed = SimEditor::zed(&doc).unwrap();

    assert_eq!(vscode.buffer, jetbrains.buffer);
    assert_eq!(reconnected_zed.buffer, jetbrains.buffer);
    assert!(reconnected_zed.buffer.contains("after zed disconnect"));

    jetbrains.close().unwrap();
    vscode.close().unwrap();
    reconnected_zed.close().unwrap();
    drop(dir);
}

// -------- Slice 3: tmux + integrated system --------

#[test]
fn integrated_editor_edit_routes_drains_under_drain_owner_gate_and_broadcasts_back() {
    // #swint Slice 4: editor edit → queue trigger → route dispatch → drain-owner
    // gate (#kp5z) → controller drain → document update → broadcast back to
    // editors, with the stuck-handoff reaper gating ownership under multi-owner
    // contention. Connects the SimEditor seam to the existing
    // route/controller actor model and the public `drain_owner` lease.
    let disk = editor_baseline_doc();
    let (dir, doc) = editor_project(&disk);
    let doc_key = doc.to_string_lossy().to_string();

    let mut owner_editor = SimEditor::jetbrains(&doc).unwrap();
    let mut observer_editor = SimEditor::vscode(&doc).unwrap();

    // 1. The user queues a follow-up by typing an unsaved queue item in the editor.
    let queued = editor_doc_with_backlog("- [ ] [#queued-followup] do the next step\n");
    owner_editor.type_unsaved(&queued).unwrap();
    let reconciliation = owner_editor.resolve().unwrap();
    assert_eq!(
        reconciliation.authority,
        DocAuthority::EditorBuffer,
        "the queued edit is authoritative over stale disk"
    );
    assert!(reconciliation.content.contains("#queued-followup"));

    // 2. The queue trigger routes to the owner pane and is accepted + proven.
    let mut world = SimWorld::new(20260611 + 1);
    world.doc = reconciliation.content.clone();
    world.snapshot = reconciliation.content.clone();
    world.apply(SimCommand::SupervisorReady).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    world.apply(SimCommand::ProveDispatchAccepted).unwrap();
    assert_eq!(
        world.coverage.route_dispatch_proofs, 1,
        "the queued trigger dispatched to the owner pane and proved acceptance"
    );

    // 3. Drain-owner gate (#kp5z): a self-driving loop owns the drain (fresh
    //    lease), so the supervisor must NOT double-inject (SkipSelfDrivingLoopOwner).
    agent_doc_queue_io::drain_owner::refresh_drain_owner_lease(
        &doc_key,
        agent_doc_queue_io::drain_owner::DRAIN_OWNER_CLAUDE_LOOP,
    )
    .unwrap();
    let lease = agent_doc_queue_io::drain_owner::read_drain_owner_lease(&doc_key)
        .expect("drain-owner lease present after refresh");
    assert!(
        agent_doc_queue_io::drain_owner::fresh_drain_owner_lease(&doc_key, lease.heartbeat_secs)
            .is_some(),
        "a fresh drain-owner lease must gate the supervisor drain to the loop owner"
    );

    // 4. The owning loop drains: apply the response and commit the document model.
    world.apply(SimCommand::CaptureResponse).unwrap();
    world.apply(SimCommand::ApplyCapturedResponse).unwrap();
    world.apply(SimCommand::Commit).unwrap();
    assert_eq!(world.phase, CyclePhase::Committed);
    assert!(
        world.snapshot.contains("#queued-followup"),
        "the editor-queued edit survived the route + drain"
    );

    // 5. Stuck-handoff reaper under multi-owner contention: a stale generation can
    //    neither hand off nor reap while the current owner holds the drain.
    world.apply(SimCommand::AdminHandoffStale).unwrap();
    assert_eq!(
        world.coverage.admin_handoffs, 0,
        "a stale-generation handoff must be rejected under contention"
    );
    world.apply(SimCommand::AdminReapStale).unwrap();
    assert_eq!(
        world.coverage.admin_reaps, 0,
        "a stale-generation reap must be rejected under contention"
    );
    assert!(world.coverage.stale_generation_blocks >= 2);

    // 6. The controller saved the committed document; the CRDT relay carries it
    //    back to both editors — they reload and converge on the committed state,
    //    clean but still editor-authoritative while attached.
    std::fs::write(&doc, &world.snapshot).unwrap();
    owner_editor.reload_from_disk().unwrap();
    observer_editor.reload_from_disk().unwrap();
    assert_eq!(owner_editor.buffer, world.snapshot);
    assert_eq!(observer_editor.buffer, world.snapshot);
    assert_eq!(
        owner_editor.resolve().unwrap().authority,
        DocAuthority::EditorBuffer,
        "after relay-back the owner editor remains authoritative while attached"
    );
    assert_eq!(
        observer_editor.resolve().unwrap().authority,
        DocAuthority::EditorBuffer,
        "the observer editor also converges through the relay while attached"
    );

    // The loop terminates: release the drain-owner lease back to the supervisor.
    agent_doc_queue_io::drain_owner::clear_drain_owner_lease(&doc_key);
    assert!(
        agent_doc_queue_io::drain_owner::read_drain_owner_lease(&doc_key).is_none(),
        "clearing the lease hands the drain back to the supervisor"
    );
    drop(dir);
}

#[test]
fn simworld_jb_run_and_clear_share_codex_enter_submit_contract() {
    // #jbcodexsubmit: JB `Run Agent Doc` and `Clear Session Context` both route
    // through the shared live-pane submit primitive. SimWorld drives the two
    // operator-facing actions, then pins the production helper's submit contract.
    let mut world = SimWorld::new(20260616);
    world.apply(SimCommand::SupervisorReady).unwrap();
    world.apply(SimCommand::DispatchOperatorPrompt).unwrap();
    world.apply(SimCommand::SessionClear).unwrap();

    assert_eq!(world.coverage.route_dispatch_acceptances, 1);
    assert_eq!(world.coverage.session_clears, 1);
    assert_eq!(
        agent_doc_tmux_commands::tmux_submit_mode_for_harness("codex"),
        "tmux_text_enter"
    );
    assert_eq!(
        agent_doc_tmux_commands::tmux_submit_transform_for_harness("codex"),
        "tmux_text_enter"
    );
    assert_eq!(
        agent_doc_tmux_commands::tmux_submit_key_for_harness("codex"),
        "Enter"
    );
    assert_eq!(
        agent_doc_tmux_commands::tmux_submit_mode_for_harness("claude"),
        "tmux_text_enter"
    );
    assert_eq!(
        agent_doc_tmux_commands::tmux_submit_mode_for_harness("opencode"),
        "tmux_text_enter"
    );
}

/// Deterministic SimWorld for the cross-document supervisor contamination class
/// (`#xdocsuper1`/`#xdocsuper2`/`#xdocsuper3`).
///
/// Models ONE route-owned supervisor process per pane/session hosting response
/// cycles in-process, backed by the *real* production state backbone
/// (`agent_doc_state_backbone::EventLedger`). The supervisor's
/// per-document in-memory state is exactly the backbone projection, so a
/// document switch / fresh host that fails to reset would surface here as a
/// sibling/stale overlay leaking into the next cycle — no live tmux required.
mod hosting_sim {
    use agent_doc_state_backbone::{
        EventLedger, RejectedStaleEvent, StateDomain, StateEvent, StateFact, StateOwner,
    };

    /// A single route-owned supervisor pane hosting documents in-process over an
    /// event-sourced backbone. `pane_session` is the stable pane/session this
    /// supervisor owns; `lease_epoch` is its supervisor-lease incarnation.
    struct HostingSimWorld {
        ledger: EventLedger,
        pane_session: String,
        lease_epoch: u64,
        next_event: u64,
    }

    impl HostingSimWorld {
        fn new(pane_session: &str) -> Self {
            Self {
                ledger: EventLedger::new(),
                pane_session: pane_session.to_string(),
                lease_epoch: 1,
                next_event: 0,
            }
        }

        fn event_id(&mut self, label: &str) -> String {
            self.next_event += 1;
            format!("{label}-{}", self.next_event)
        }

        /// The supervisor begins hosting (or switches to) `document_hash` on its
        /// pane. This is the FIRST thing the host loop does on every handoff —
        /// it advances the document's hosting epoch and drops any stale queue
        /// overlay by construction.
        fn host(&mut self, document_hash: &str) {
            let id = self.event_id("host");
            let pane_session = self.pane_session.clone();
            let lease_epoch = self.lease_epoch;
            self.ledger.append(StateEvent::new(
                id,
                StateFact::SupervisorHosting {
                    document_hash: document_hash.to_string(),
                    pane_session,
                    lease_epoch,
                },
            ));
        }

        /// Simulate a supervisor lease re-incarnation (e.g. an in-place recycle /
        /// stale-CRDT replay boundary) before the next host on the same pane.
        fn bump_lease(&mut self) {
            self.lease_epoch += 1;
        }

        /// The current hosting epoch a live producer stamps onto queue facts.
        fn hosting_epoch(&self, document_hash: &str) -> Option<u64> {
            self.ledger.document_hosting_epoch(document_hash)
        }

        /// Select a free-text queue head for the document under the CURRENT
        /// hosting epoch (the normal in-hosting path).
        fn select_head(&mut self, document_hash: &str, node_key: &str) {
            let hosting_epoch = self.hosting_epoch(document_hash);
            let id = self.event_id("sel");
            self.ledger.append(StateEvent::new(
                id,
                StateFact::QueueHeadSelected {
                    document_hash: document_hash.to_string(),
                    node_key: node_key.to_string(),
                    backlog_id: None,
                    prompt_text: None,
                    drainable: true,
                    hosting_epoch,
                },
            ));
        }

        /// Answer (complete) a queue head for the document under the CURRENT
        /// hosting epoch.
        fn complete_head(&mut self, document_hash: &str, node_key: &str) {
            let hosting_epoch = self.hosting_epoch(document_hash);
            let id = self.event_id("done");
            self.ledger.append(StateEvent::new(
                id,
                StateFact::QueueHeadCompleted {
                    document_hash: document_hash.to_string(),
                    node_key: node_key.to_string(),
                    backlog_id: None,
                    hosting_epoch,
                },
            ));
        }

        /// Replay a stale answered-head fact stamped with an OLD hosting epoch —
        /// the contamination vector (stale CRDT / answered-head residue).
        fn replay_stale_complete(
            &mut self,
            document_hash: &str,
            node_key: &str,
            stale_hosting_epoch: u64,
        ) {
            let id = self.event_id("stale");
            self.ledger.append(StateEvent::new(
                id,
                StateFact::QueueHeadCompleted {
                    document_hash: document_hash.to_string(),
                    node_key: node_key.to_string(),
                    backlog_id: None,
                    hosting_epoch: Some(stale_hosting_epoch),
                },
            ));
        }

        fn completed_heads(&self, document_hash: &str) -> Vec<String> {
            self.ledger
                .project_document(document_hash)
                .map(|doc| doc.queue.completed_heads.iter().cloned().collect())
                .unwrap_or_default()
        }

        fn active_head(&self, document_hash: &str) -> Option<String> {
            self.ledger
                .project_document(document_hash)
                .and_then(|doc| doc.queue.active_head)
        }

        fn rejected_stale(&self, document_hash: &str) -> Vec<RejectedStaleEvent> {
            self.ledger
                .project_document(document_hash)
                .map(|doc| doc.rejected_stale_events)
                .unwrap_or_default()
        }
    }

    #[test]
    fn route_owned_supervisor_switch_reads_target_without_source_overlay() {
        // (a) One route-owned supervisor hosts doc A, answers a free-text head,
        // then is handed doc B on the SAME pane. B's first cycle must read B's
        // on-disk state with NO A overlay, and A's state must be untouched.
        let mut world = HostingSimWorld::new("%73:b26b9957");
        world.host("doc-a");
        world.select_head("doc-a", "a-free-text-head");
        world.complete_head("doc-a", "a-free-text-head");
        assert_eq!(world.completed_heads("doc-a"), vec!["a-free-text-head"]);

        // Switch to doc B.
        world.host("doc-b");
        world.select_head("doc-b", "b-free-text-head");

        // B sees only its own head; A is not contaminated.
        assert_eq!(
            world.active_head("doc-b").as_deref(),
            Some("b-free-text-head")
        );
        assert!(
            world.completed_heads("doc-b").is_empty(),
            "doc-b's first cycle must not inherit doc-a's answered head"
        );
        assert_eq!(
            world.completed_heads("doc-a"),
            vec!["a-free-text-head"],
            "switching the pane to doc-b must not mutate doc-a"
        );
    }

    #[test]
    fn contamination_regression_no_answered_head_reinjection_after_switch() {
        // (b) Contamination regression: after a switch (or stale same-document
        // re-host), a sibling/stale in-memory overlay must not re-inject an
        // already-answered free-text head — the live_prompt_drift / answered-head
        // residue class. We drive the actual stale-fact replay vector.
        let mut world = HostingSimWorld::new("%73:b26b9957");

        // Same-document stale-CRDT replay boundary: host doc-a, answer a head,
        // then the supervisor lease re-incarnates and re-hosts doc-a.
        world.host("doc-a");
        world.select_head("doc-a", "answered-head");
        world.complete_head("doc-a", "answered-head");
        let stale_epoch = world.hosting_epoch("doc-a").unwrap();

        world.bump_lease();
        world.host("doc-a");
        assert!(
            world.completed_heads("doc-a").is_empty(),
            "fresh host must drop the prior hosting's answered-head residue"
        );

        // The stale supervisor replays the answered-head fact at the OLD epoch
        // (the contamination vector). It must be rejected, not re-injected.
        world.replay_stale_complete("doc-a", "answered-head", stale_epoch);
        assert!(
            world.completed_heads("doc-a").is_empty(),
            "stale answered-head replay must not re-appear after the re-host"
        );
        assert_eq!(
            world.rejected_stale("doc-a"),
            vec![RejectedStaleEvent {
                domain: StateDomain::Queue,
                owner: StateOwner::QueueOrchestrator,
            }],
            "the stale-epoch queue replay must be recorded as rejected"
        );

        // Cross-document sibling contamination: switching the same pane to a
        // second document must give doc-b a clean overlay (no doc-a head), and a
        // doc-a stale replay must never reach doc-b's projection (facts are keyed
        // by document_hash, and doc-b's switch reset already cleared its overlay).
        world.host("doc-b");
        world.replay_stale_complete("doc-a", "answered-head", stale_epoch);
        assert!(
            world.completed_heads("doc-b").is_empty(),
            "a sibling document's stale overlay must not contaminate doc-b"
        );
    }

    #[test]
    fn completed_queue_lifecycle_rejects_late_selection_reactivation() {
        // A late Markdown/poller observation may rediscover the old queue row
        // after the semantic closeout completed. The reactive lifecycle is
        // authoritative: Completed is terminal within the hosting epoch, so
        // the delayed Selected fact cannot make the item active again.
        let mut world = HostingSimWorld::new("%19:neutral-session");
        world.host("neutral-document");
        world.select_head("neutral-document", "tracked-head");
        world.complete_head("neutral-document", "tracked-head");

        world.select_head("neutral-document", "tracked-head");

        assert_eq!(
            world.completed_heads("neutral-document"),
            vec!["tracked-head"]
        );
        assert_eq!(
            world.active_head("neutral-document"),
            None,
            "late textual projection must not reactivate a completed queue item"
        );
    }

    #[test]
    fn deterministic_replay_is_stable_across_runs() {
        // SimWorld determinism: the same scenario yields byte-identical projection
        // JSON on every run (no live tmux, no wall-clock, no RNG dependence).
        fn run() -> String {
            let mut world = HostingSimWorld::new("%5:session-x");
            world.host("doc-a");
            world.select_head("doc-a", "h1");
            world.complete_head("doc-a", "h1");
            world.bump_lease();
            world.host("doc-a");
            world.select_head("doc-a", "h2");
            world.host("doc-b");
            world.select_head("doc-b", "h3");
            let a = world.ledger.project_document("doc-a").unwrap();
            let b = world.ledger.project_document("doc-b").unwrap();
            format!(
                "{}\n{}",
                serde_json::to_string(&a).unwrap(),
                serde_json::to_string(&b).unwrap()
            )
        }
        assert_eq!(run(), run());
    }
}

/// Deterministic SimWorld for the CRDT-authority state machine (`#crdtauth1`).
///
/// Models the additive authority layer
/// (`agent_doc_document_realtime::crdt_authority`) riding the EXISTING per-document
/// hosting-epoch backbone (`agent_doc_state_backbone::EventLedger`). The authority follows
/// the live editor: a document with a proven live editor-IPC transport is
/// `MultiReplica` (durable-projection semantics); a headless / detached / stale
/// document is `GitAuthoritative` (ephemeral CRDT). Per-document isolation is
/// derived from the same backbone projection that `#xdocsuper1/3` isolates, so a
/// stale-overlay replay for one document cannot flip another's authority — no live
/// editor / tmux required.
mod crdt_authority_sim {
    use agent_doc_document_realtime::crdt_authority::CrdtAuthority;
    use agent_doc_document_realtime::crdt_authority::authority_for_document;
    use agent_doc_state_backbone::{
        ActorLifecycleEvent, EventLedger, StateEvent, StateFact, StateOwner,
    };

    /// A single route-owned supervisor pane hosting documents in-process over an
    /// event-sourced backbone, exercising the authority transitions. `attach` and
    /// `detach` are the authority transitions; both ride the hosting-epoch
    /// substrate that the supervisor host loop drives.
    struct AuthoritySimWorld {
        ledger: EventLedger,
        pane_session: String,
        lease_epoch: u64,
        editor_generation: u64,
        next_event: u64,
    }

    impl AuthoritySimWorld {
        fn new(pane_session: &str) -> Self {
            Self {
                ledger: EventLedger::new(),
                pane_session: pane_session.to_string(),
                lease_epoch: 1,
                editor_generation: 0,
                next_event: 0,
            }
        }

        fn event_id(&mut self, label: &str) -> String {
            self.next_event += 1;
            format!("{label}-{}", self.next_event)
        }

        /// The supervisor begins hosting (or switches to) `document_hash` — the
        /// hosting-epoch transition every handoff runs first.
        fn host(&mut self, document_hash: &str) {
            let id = self.event_id("host");
            let pane_session = self.pane_session.clone();
            let lease_epoch = self.lease_epoch;
            self.ledger.append(StateEvent::new(
                id,
                StateFact::SupervisorHosting {
                    document_hash: document_hash.to_string(),
                    pane_session,
                    lease_epoch,
                },
            ));
        }

        /// Editor `attach`: a live editor-IPC bridge replica registers for the
        /// document (advances the editor generation). This is the
        /// Detached → MultiReplica authority transition.
        fn attach_editor(&mut self, document_hash: &str) {
            self.editor_generation += 1;
            let generation = self.editor_generation;
            let id = self.event_id("attach");
            self.ledger.append(StateEvent::new(
                id,
                StateFact::OwnerGenerationChanged {
                    document_hash: document_hash.to_string(),
                    owner: StateOwner::EditorIpcBridge,
                    generation,
                },
            ));
        }

        /// The live editor queues + applies a patch under the current generation
        /// (normal multi-replica coordination, proving the editor replica is the
        /// live medium).
        fn editor_synced_patch(&mut self, document_hash: &str, patch_id: &str) {
            let generation = self.editor_generation;
            let queued = self.event_id("queued");
            self.ledger.append(StateEvent::new(
                queued,
                StateFact::EditorPatchQueued {
                    document_hash: document_hash.to_string(),
                    patch_id: patch_id.to_string(),
                    actor_generation: generation,
                },
            ));
            let applied = self.event_id("applied");
            self.ledger.append(StateEvent::new(
                applied,
                StateFact::EditorPatchApplied {
                    document_hash: document_hash.to_string(),
                    patch_id: patch_id.to_string(),
                    actor_generation: generation,
                },
            ));
        }

        /// Editor `detach` via a stale-listener / dead-pid demote: the supervisor
        /// abandons the editor replica for this turn and falls back to a disk
        /// write (the terminal force-disk fallback). This is the
        /// MultiReplica → GitAuthoritative authority transition, mirroring the
        /// `disk_write_permitted_for_file` Detached fallback.
        fn editor_force_disk_fallback(
            &mut self,
            document_hash: &str,
            patch_id: &str,
            reason: &str,
        ) {
            let generation = self.editor_generation;
            // The fallback needs an existing patch to terminalize.
            let queued = self.event_id("fb-queued");
            self.ledger.append(StateEvent::new(
                queued,
                StateFact::EditorPatchQueued {
                    document_hash: document_hash.to_string(),
                    patch_id: patch_id.to_string(),
                    actor_generation: generation,
                },
            ));
            let fb = self.event_id("fb");
            self.ledger.append(StateEvent::new(
                fb,
                StateFact::ForceDiskFallbackRecorded {
                    document_hash: document_hash.to_string(),
                    patch_id: patch_id.to_string(),
                    actor_generation: generation,
                    reason: reason.to_string(),
                },
            ));
        }

        /// Record a benign supervisor lifecycle fact for the document (used to
        /// give a document a projection without an editor transport).
        fn supervisor_alive(&mut self, document_hash: &str) {
            let id = self.event_id("sup");
            self.ledger.append(StateEvent::new(
                id,
                StateFact::ActorLifecycleObserved {
                    document_hash: document_hash.to_string(),
                    owner: StateOwner::Supervisor,
                    generation: 0,
                    event: ActorLifecycleEvent::ReadyObserved,
                },
            ));
        }

        fn authority(&self, document_hash: &str) -> CrdtAuthority {
            authority_for_document(&self.ledger, document_hash)
        }
    }

    #[test]
    fn attach_routes_to_multi_replica_detach_routes_to_git_authoritative() {
        // Coverage 1: attach → MultiReplica; detach → GitAuthoritative (ephemeral).
        let mut world = AuthoritySimWorld::new("%73:auth");
        world.host("doc-a");

        // Headless before any editor attaches: git-authoritative + ephemeral.
        world.supervisor_alive("doc-a");
        let headless = world.authority("doc-a");
        assert_eq!(headless, CrdtAuthority::GitAuthoritative);
        assert!(
            headless.crdt_is_ephemeral(),
            "a headless document rebuilds an ephemeral CRDT from git"
        );

        // attach → MultiReplica (durable projection).
        world.attach_editor("doc-a");
        world.editor_synced_patch("doc-a", "p1");
        let attached = world.authority("doc-a");
        assert_eq!(attached, CrdtAuthority::MultiReplica);
        assert!(
            attached.disk_is_durable_projection(),
            "with a live editor, disk is a boundary-checkpointed durable projection"
        );
        assert!(!attached.crdt_is_ephemeral());

        // detach (force-disk fallback) → GitAuthoritative (ephemeral) again.
        world.editor_force_disk_fallback("doc-a", "p2", "no_ack");
        let detached = world.authority("doc-a");
        assert_eq!(detached, CrdtAuthority::GitAuthoritative);
        assert!(
            detached.crdt_is_ephemeral(),
            "after the editor replica is abandoned the CRDT is ephemeral again"
        );
    }

    #[test]
    fn stale_listener_dead_pid_demote_routes_to_git_authoritative() {
        // Coverage 2: a stale listener / dead pid demote routes to
        // GitAuthoritative, mirroring the `disk_write_permitted_for_file`
        // Detached fallback. We model the demote as the terminal force-disk
        // fallback the supervisor records when no live editor sits behind the
        // listener.
        let mut world = AuthoritySimWorld::new("%73:auth");
        world.host("doc-a");
        world.attach_editor("doc-a");
        assert_eq!(world.authority("doc-a"), CrdtAuthority::MultiReplica);

        // The listener turns out stale (no live editor): the supervisor falls
        // back to disk. Authority must demote to git-authoritative so the write
        // routes to the controller-host disk path instead of wedging on no_ack.
        world.editor_force_disk_fallback("doc-a", "p1", "stale_listener_no_ack");
        let demoted = world.authority("doc-a");
        assert_eq!(
            demoted,
            CrdtAuthority::GitAuthoritative,
            "a stale listener / dead pid demote routes to git-authoritative"
        );
        assert!(demoted.crdt_is_ephemeral());
    }

    #[test]
    fn per_document_isolation_overlay_replay_for_doc_a_does_not_change_doc_b() {
        // Coverage 3: per-document isolation. A hosting-epoch / overlay replay for
        // doc A must not change doc B's authority. Authority derives from each
        // document's own projection, so a stale doc-A replay can never reach
        // doc-B's projection (facts are keyed by document_hash; #xdocsuper1/3
        // already isolates the overlay).
        let mut world = AuthoritySimWorld::new("%73:auth");

        // doc-a is multi-replica (live editor); doc-b is headless.
        world.host("doc-a");
        world.attach_editor("doc-a");
        world.editor_synced_patch("doc-a", "a1");
        assert_eq!(world.authority("doc-a"), CrdtAuthority::MultiReplica);

        world.host("doc-b");
        world.supervisor_alive("doc-b");
        assert_eq!(
            world.authority("doc-b"),
            CrdtAuthority::GitAuthoritative,
            "doc-b has no editor — it is git-authoritative"
        );

        // A stale-overlay replay for doc-a (its old hosting epoch re-emits an
        // already-applied editor patch) must not flip doc-b's authority.
        world.editor_synced_patch("doc-a", "a1-replay");
        assert_eq!(
            world.authority("doc-b"),
            CrdtAuthority::GitAuthoritative,
            "a doc-a overlay replay must NOT flip doc-b to multi-replica"
        );

        // Symmetric direction: doc-a stays multi-replica; doc-b's headlessness did
        // not bleed into it.
        assert_eq!(world.authority("doc-a"), CrdtAuthority::MultiReplica);

        // And re-hosting doc-b (a hosting-epoch bump) on the same pane does not
        // change doc-a's authority either.
        world.host("doc-b");
        assert_eq!(world.authority("doc-a"), CrdtAuthority::MultiReplica);
    }

    #[test]
    fn git_authoritative_is_ephemeral_multi_replica_is_durable_projection() {
        // Coverage 4: GitAuthoritative ⇒ ephemeral CRDT (no durable-authority
        // assumption); MultiReplica ⇒ durable-projection semantics. Asserted as a
        // total invariant over both reachable authority states.
        let git = CrdtAuthority::GitAuthoritative;
        assert!(git.crdt_is_ephemeral());
        assert!(!git.disk_is_durable_projection());
        assert!(!git.editor_attached());

        let multi = CrdtAuthority::MultiReplica;
        assert!(!multi.crdt_is_ephemeral());
        assert!(multi.disk_is_durable_projection());
        assert!(multi.editor_attached());

        // The two predicates partition the authority states (mutually exclusive,
        // exhaustive) — no third durability mode.
        for authority in [CrdtAuthority::GitAuthoritative, CrdtAuthority::MultiReplica] {
            assert_ne!(
                authority.crdt_is_ephemeral(),
                authority.disk_is_durable_projection(),
                "exactly one durability mode holds per authority state"
            );
        }
    }

    #[test]
    fn unknown_document_is_git_authoritative_failsafe() {
        // A document the supervisor has never hosted with an editor is headless
        // until proven otherwise — fail-safe to the cheapest, zero-stale state.
        let world = AuthoritySimWorld::new("%73:auth");
        assert_eq!(
            world.authority("never-seen"),
            CrdtAuthority::GitAuthoritative
        );
    }

    #[test]
    fn deterministic_replay_is_stable_across_runs() {
        // SimWorld determinism: byte-identical authority decisions on every run
        // (no live tmux, no wall-clock, no RNG).
        fn run() -> String {
            let mut world = AuthoritySimWorld::new("%5:session-x");
            world.host("doc-a");
            world.attach_editor("doc-a");
            world.editor_synced_patch("doc-a", "p1");
            world.host("doc-b");
            world.supervisor_alive("doc-b");
            format!(
                "a={:?} b={:?}",
                world.authority("doc-a"),
                world.authority("doc-b")
            )
        }
        assert_eq!(run(), run());
    }
}

/// Deterministic SimWorld for the multi-editor relay hub + awareness
/// (`#crdtauth4`, plan phase 5) and disk demotion (plan phase 6).
///
/// Models a single supervisor-hosted canonical replica with N editor replicas
/// registered through the star-topology relay
/// (`agent_doc_document_realtime::crdt_relay::RelayHub`). Fan-out packets can be held
/// in flight and delivered out of order to model propagation lag — no live editor
/// / tmux / socket required. Convergence, the live-cut commit barrier, offline →
/// reconnect catch-up, and unique-client-id enforcement are all asserted
/// deterministically.
mod crdt_relay_sim {
    use agent_doc_document_realtime::crdt_authority::CrdtAuthority;
    use agent_doc_document_realtime::crdt_relay::{AwarenessState, RelayHub, mint_client_id};

    /// A supervisor pane hosting one document over the relay hub, plus an in-flight
    /// packet queue modeling the editor↔supervisor network. Delivery order is
    /// caller-controlled so lag / reordering is deterministic.
    struct RelaySimWorld {
        hub: RelayHub,
        /// Pending fan-out deliveries: `(target_client_id, update_bytes)`.
        inflight: Vec<(u64, Vec<u8>)>,
    }

    impl RelaySimWorld {
        fn new(canonical_id: u64) -> Self {
            Self {
                hub: RelayHub::new(canonical_id),
                inflight: Vec::new(),
            }
        }

        /// Attach an editor identified by a stable string identity: mint its
        /// stable client-id and register it. Returns the minted id.
        fn attach(&mut self, identity: &str) -> u64 {
            let id = mint_client_id(identity);
            self.hub
                .register(id)
                .unwrap_or_else(|e| panic!("attach {identity}: {e}"));
            id
        }

        /// An editor edit that relays + broadcasts immediately (the normal live path).
        fn edit_now(&mut self, id: u64, offset: u32, delete_len: u32, insert: &str) {
            self.hub
                .apply_local(id, offset, delete_len, insert)
                .unwrap();
        }

        /// An editor edit relayed to the hub but whose fan-out packets to peers are
        /// held in flight (supervisor→peer lag).
        fn edit_lagged(&mut self, id: u64, offset: u32, delete_len: u32, insert: &str) {
            self.hub.local_edit(id, offset, delete_len, insert).unwrap();
            let packet = self.hub.relay_capture(id).unwrap();
            for target in packet.targets {
                self.inflight.push((target, packet.update.clone()));
            }
        }

        /// An editor edit applied to its OWN replica only — NOT relayed to the hub
        /// (an un-propagated op; the editor→supervisor direction is in flight).
        fn edit_local_only(&mut self, id: u64, offset: u32, delete_len: u32, insert: &str) {
            self.hub.local_edit(id, offset, delete_len, insert).unwrap();
        }

        /// Deliver all in-flight packets in REVERSE submission order (out of order).
        fn deliver_reversed(&mut self) {
            let mut pending = std::mem::take(&mut self.inflight);
            pending.reverse();
            for (target, update) in pending {
                self.hub.deliver(target, &update).unwrap();
            }
        }

        fn append_len(&self, id: u64) -> u32 {
            self.hub.member_text(id).unwrap_or_default().chars().count() as u32
        }
    }

    #[test]
    fn multi_replica_fan_out_reaches_all_other_editors() {
        // Coverage: an update from one replica reaches every other live replica.
        let mut world = RelaySimWorld::new(1);
        let a = world.attach("intellij:a");
        let b = world.attach("vscode:b");
        let c = world.attach("intellij:c");

        world.edit_now(a, 0, 0, "shared");
        assert_eq!(world.hub.canonical_text(), "shared");
        for id in [a, b, c] {
            assert_eq!(
                world.hub.member_text(id).unwrap(),
                "shared",
                "fan-out reached replica {id}"
            );
        }
    }

    #[test]
    fn convergence_under_lag_out_of_order_delivery() {
        // Coverage: delayed / out-of-order fan-out still converges (yrs causal
        // buffering at the hub-delivery layer).
        let mut world = RelaySimWorld::new(1);
        let a = world.attach("editor:a");
        let b = world.attach("editor:b");

        // Two dependent edits from `a`, packets held in flight.
        world.edit_lagged(a, 0, 0, "first");
        let len = world.append_len(a);
        world.edit_lagged(a, len, 0, " second");

        // `b` has not seen either yet (lagged).
        assert_ne!(
            world.hub.member_text(b).unwrap(),
            world.hub.canonical_text()
        );

        // Deliver REVERSED (the dependent op before its dependency) — converges.
        world.deliver_reversed();
        assert_eq!(
            world.hub.member_text(b).unwrap(),
            world.hub.canonical_text(),
            "out-of-order delivery self-heals once causal deps arrive"
        );
        assert!(world.hub.member_text(b).unwrap().contains("first second"));
    }

    #[test]
    fn commit_barrier_consistent_cut_with_three_replicas_no_deadlock() {
        // Coverage: the commit barrier with N=3 replicas captures all LIVE
        // editors' ops; a disconnected editor does not deadlock the barrier and
        // contributes its ops at next sync.
        let mut world = RelaySimWorld::new(1);
        let a = world.attach("editor:a");
        let b = world.attach("editor:b");
        let c = world.attach("editor:c");

        // Each editor types locally WITHOUT relaying (un-propagated ops — the
        // canonical replica does not hold them yet).
        world.edit_local_only(a, 0, 0, "AAA");
        world.edit_local_only(b, 0, 0, "BBB");
        world.edit_local_only(c, 0, 0, "CCC");
        // Editor c disconnects with its op un-flushed (slow / offline editor).
        world.hub.disconnect(c);

        // The barrier captures the two LIVE editors and does NOT block on c.
        assert!(
            world
                .hub
                .commit_barrier_under_authority(CrdtAuthority::MultiReplica)
                .unwrap(),
            "barrier completes a consistent cut of the live replicas"
        );
        let cut = world.hub.canonical_text();
        assert!(cut.contains("AAA") && cut.contains("BBB"));
        assert!(
            !cut.contains("CCC"),
            "the disconnected editor's op is NOT in this checkpoint"
        );

        // c contributes its op at the next checkpoint after reconnect — no loss.
        world.hub.reconnect(c).unwrap();
        assert!(world.hub.commit_barrier().unwrap());
        assert!(world.hub.canonical_text().contains("CCC"));
    }

    #[test]
    fn offline_then_reconnect_converges_no_data_loss() {
        // Coverage: a replica that missed updates while offline converges on
        // reconnect via state-vector catch-up; its offline local edits survive.
        let mut world = RelaySimWorld::new(1);
        let a = world.attach("editor:a");
        let b = world.attach("editor:b");

        world.hub.disconnect(b);
        // `a` edits while `b` is offline (b misses the broadcast).
        world.edit_now(a, 0, 0, "online-edit ");
        // `b` edits locally while offline (its own replica only).
        world.edit_local_only(b, 0, 0, "offline-edit ");

        world.hub.reconnect(b).unwrap();
        let tb = world.hub.member_text(b).unwrap();
        assert!(tb.contains("online-edit"), "missed update caught up");
        assert!(tb.contains("offline-edit"), "offline local edit preserved");
        assert_eq!(
            tb,
            world.hub.canonical_text(),
            "reconnected replica converged with canonical"
        );
    }

    #[test]
    fn duplicate_client_id_is_rejected() {
        // Coverage: a client-id collision is a hard error (corruption per the
        // unique-stable-client-id rule).
        let mut world = RelaySimWorld::new(1);
        let _a = world.attach("editor:a");
        // The SAME identity mints the SAME id → registering twice collides.
        let dup = mint_client_id("editor:a");
        assert!(
            world.hub.register(dup).is_err(),
            "re-registering an existing client-id must be rejected"
        );
        // Colliding with the canonical id is also rejected.
        assert!(world.hub.register(1).is_err());
    }

    #[test]
    fn awareness_is_ephemeral_and_not_part_of_the_document() {
        // Coverage: presence is a separate ephemeral channel; it never touches the
        // document text and is expired on deregister.
        let mut world = RelaySimWorld::new(1);
        let a = world.attach("editor:a");
        let b = world.attach("editor:b");
        world.edit_now(a, 0, 0, "doc");

        world.hub.set_awareness(
            a,
            AwarenessState {
                cursor: Some(3),
                user: Some("alice".into()),
                ..Default::default()
            },
        );
        world.hub.set_awareness(
            b,
            AwarenessState {
                cursor: Some(0),
                ..Default::default()
            },
        );
        assert_eq!(world.hub.awareness_snapshot().len(), 2);
        // Awareness did not alter the document text.
        assert_eq!(world.hub.canonical_text(), "doc");

        // Deregister expires presence (never persisted, never committed).
        world.hub.deregister(b);
        let snap = world.hub.awareness_snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].0, a);
    }

    #[test]
    fn file_cache_conflict_backpressure_applies_coalesced_latest_once_after_ack() {
        use agent_doc_document_realtime::write_policy::{
            CrdtWriteAdmission, decide_crdt_write_admission,
        };

        let baseline = "# Session\n\nCurrent.\n";
        let first = "# Session\n\nCurrent.\n\n### Re: once\n\nApplied.\n";
        let latest = "# Session\n\nCurrent.\n\n### Re: once\n\nApplied and normalized.\n";
        let mut world = RelaySimWorld::new(1);
        let editor = world.attach("intellij:file-cache-conflict");
        world.edit_now(editor, 0, 0, baseline);

        world
            .hub
            .apply_canonical_replace(baseline, first)
            .expect("first canonical write");
        assert_eq!(
            decide_crdt_write_admission(world.hub.delivery_converged()),
            CrdtWriteAdmission::WaitForDeliveryProjection,
            "a second write must not stack behind an unacknowledged editor delivery",
        );
        assert_eq!(
            world.hub.pending_updates(editor).unwrap().len(),
            1,
            "the editor-visible ACK frontier still has one delivery in flight",
        );

        // While the first frontier is in flight, newer intent replaces the
        // queued target in the caller. It is not replayed through a second
        // mutation plane and is not applied until the first visible ACK.
        let coalesced_latest = latest;
        let first_delivery = world.hub.pending_updates(editor).unwrap().pop().unwrap();
        world.hub.deliver(editor, &first_delivery.update).unwrap();
        world
            .hub
            .ack_delivery(editor, &first_delivery.patch_id, first_delivery.generation)
            .unwrap();
        assert_eq!(
            decide_crdt_write_admission(world.hub.delivery_converged()),
            CrdtWriteAdmission::ApplyLatest,
        );

        world
            .hub
            .apply_canonical_replace(first, coalesced_latest)
            .expect("coalesced latest write");
        let latest_delivery = world.hub.pending_updates(editor).unwrap().pop().unwrap();
        world.hub.deliver(editor, &latest_delivery.update).unwrap();
        world
            .hub
            .ack_delivery(
                editor,
                &latest_delivery.patch_id,
                latest_delivery.generation,
            )
            .unwrap();

        assert!(world.hub.delivery_converged());
        assert_eq!(world.hub.canonical_text(), coalesced_latest);
        assert_eq!(world.hub.member_text(editor).unwrap(), coalesced_latest);
        assert_eq!(
            world.hub.canonical_text().matches("### Re: once").count(),
            1
        );
    }

    #[test]
    fn preflight_boundary_coalesces_legacy_whole_document_replay_via_crdt() {
        use agent_doc_document_realtime::write_policy::coalesce_exact_document_replay;

        let canonical = concat!(
            "---\nagent_doc_session: sim\n---\n\n",
            "<!-- agent:exchange patch=append -->\noperator text\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:done -->\n<!-- /agent:done -->\n",
        );
        let replayed = canonical.repeat(2);
        let mut world = RelaySimWorld::new(1);
        let editor = world.attach("intellij:legacy-replay");
        world.edit_now(editor, 0, 0, &replayed);

        let replayed_canonical = world.hub.canonical_text();
        let coalesced = coalesce_exact_document_replay(&replayed_canonical).unwrap();
        assert_eq!(coalesced.copies, 2);
        world
            .hub
            .apply_canonical_replace(&replayed, coalesced.canonical)
            .expect("preflight convergence write");
        let delivery = world.hub.pending_updates(editor).unwrap().pop().unwrap();
        world.hub.deliver(editor, &delivery.update).unwrap();
        world
            .hub
            .ack_delivery(editor, &delivery.patch_id, delivery.generation)
            .unwrap();

        assert!(world.hub.delivery_converged());
        assert_eq!(world.hub.canonical_text(), canonical);
        assert_eq!(world.hub.member_text(editor).unwrap(), canonical);
    }

    #[test]
    fn response_cell_intent_precedes_reactive_projection_receipt() {
        let baseline = "# Session\n\noperator prompt\n";
        let response = "# Session\n\noperator prompt\n\n### Re: prompt — gpt-5\n\nDone.\n";
        let mut world = RelaySimWorld::new(1);
        let editor = world.attach("intellij:response-closeout");
        world.edit_now(editor, 0, 0, baseline);
        let mut disk_projection = baseline.to_string();

        world
            .hub
            .apply_canonical_replace(baseline, response)
            .expect("semantic response-cell projection");
        assert_eq!(
            world.hub.canonical_text(),
            response,
            "the binary-owned intent is durable before any editor receipt"
        );
        assert!(!world.hub.delivery_converged());
        assert_eq!(disk_projection, baseline);

        let delivery = world.hub.pending_updates(editor).unwrap().pop().unwrap();
        world.hub.deliver(editor, &delivery.update).unwrap();
        world
            .hub
            .ack_delivery(editor, &delivery.patch_id, delivery.generation)
            .unwrap();
        assert!(world.hub.delivery_converged());
        disk_projection = world.hub.canonical_text();

        assert_eq!(disk_projection, response);
        assert_eq!(world.hub.member_text(editor).unwrap(), response);
        assert_eq!(disk_projection.matches("### Re: prompt").count(), 1);
    }

    #[test]
    fn disk_projection_recovers_canonical_and_in_memory_wins() {
        // Coverage (phase 6): the disk projection is a recovery input; a restart
        // rebuilds the canonical replica from it, and a stale projection never
        // regresses a live replica (in-memory wins).
        let mut world = RelaySimWorld::new(1);
        let a = world.attach("editor:a");
        world.edit_now(a, 0, 0, "v1");
        let projection = world.hub.projection_bytes();

        // Restart: rebuild canonical from the recovery projection.
        let recovered = RelayHub::recover_from_projection(1, &projection).unwrap();
        assert_eq!(recovered.canonical_text(), "v1");

        // Live session advances; reconciling the STALE projection is a no-op.
        let len = world.hub.canonical_text().chars().count() as u32;
        world.edit_now(a, len, 0, " v2");
        let changed = world.hub.reconcile_disk_projection(&projection).unwrap();
        assert!(!changed);
        assert_eq!(
            world.hub.canonical_text(),
            "v1 v2",
            "in-memory replica wins"
        );
    }

    #[test]
    fn deterministic_replay_is_stable_across_runs() {
        // SimWorld determinism: byte-identical relay outcomes every run (stable
        // minted ids, no wall-clock, no RNG).
        fn run() -> String {
            let mut world = RelaySimWorld::new(1);
            let a = world.attach("editor:a");
            let b = world.attach("editor:b");
            let c = world.attach("editor:c");
            world.edit_now(a, 0, 0, "x");
            world.edit_lagged(b, 0, 0, "y");
            world.edit_lagged(c, 0, 0, "z");
            world.deliver_reversed();
            world.hub.commit_barrier().unwrap();
            format!(
                "ids={a},{b},{c} canon={} a={} b={} c={}",
                world.hub.canonical_text(),
                world.hub.member_text(a).unwrap(),
                world.hub.member_text(b).unwrap(),
                world.hub.member_text(c).unwrap(),
            )
        }
        assert_eq!(run(), run());
    }
}

/// Deterministic SimWorld for the LIVE relay-host cutover (`#crdtauth4`).
///
/// Where `crdt_relay_sim` above exercises the standalone `RelayHub` API directly,
/// this module drives the wiring the live finalize / disk paths actually call:
/// `agent_doc_crdt_relay_io` — the per-document hub registry, the
/// authority-gated finalize commit barrier (`commit_barrier_for_file_with_authority`),
/// and the authority-gated disk-demotion reconcile. It proves the LIVE seams (a)
/// gate on `CrdtAuthority::EditorAttached` vs `Detached`, (b) never allocate / touch
/// a hub on the Detached path (so headless traffic is byte-for-byte unchanged), and
/// (c) flush live replicas to a consistent cut for the EditorAttached path — all
/// keyed per-document through a real tracked path, no live editor / tmux / socket.
mod crdt_relay_host_sim {
    use agent_doc_crdt_relay_io::{
        commit_barrier_for_file_with_authority, recover_hub_from_projection, with_hub,
    };
    use agent_doc_document_realtime::crdt_authority::CrdtAuthority;
    use agent_doc_document_realtime::crdt_relay::{RelayHub, mint_client_id};
    use std::io::Write;
    use std::path::PathBuf;

    /// A throwaway tracked document under its own temp project root, so the live
    /// `crdt_relay_host` registry keys per-document via `agent_doc_fs::document_state_hash`.
    fn temp_doc(name: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let path = dir.path().join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "# {name}\n\nbody").unwrap();
        (dir, path)
    }

    #[test]
    fn live_finalize_barrier_flushes_editor_attached_but_noops_detached() {
        // EDITOR-ATTACHED doc: a live editor types a local op that has NOT been
        // relayed; the LIVE finalize barrier flushes it into the committed cut.
        let (_attached_dir, attached) = temp_doc("live-attached.md");
        let editor = mint_client_id("intellij:live-finalize");
        with_hub(&attached, |hub| {
            hub.register(editor).unwrap();
            hub.local_edit(editor, 0, 0, "keystroke").unwrap();
            assert!(
                !hub.canonical_text().contains("keystroke"),
                "not relayed yet"
            );
        })
        .unwrap();
        assert!(commit_barrier_for_file_with_authority(
            &attached,
            CrdtAuthority::MultiReplica
        ));
        with_hub(&attached, |hub| {
            assert!(
                hub.canonical_text().contains("keystroke"),
                "the live finalize barrier flushed the editor's op into the cut"
            );
        })
        .unwrap();

        // DETACHED doc (a SEPARATE document): the live barrier is a trivial no-op
        // that allocates no hub — per-document isolation + headless path unchanged.
        let (_detached_dir, detached) = temp_doc("live-detached.md");
        let hash = agent_doc_fs::document_state_hash(&detached).unwrap();
        assert!(commit_barrier_for_file_with_authority(
            &detached,
            CrdtAuthority::GitAuthoritative
        ));
        // The Detached path never touched a hub for its own document.
        let touched = with_hub(&detached, |hub| hub.live_count()).unwrap();
        assert_eq!(
            touched, 0,
            "the Detached commit barrier allocates no live replicas (hub {hash} stays empty)"
        );
    }

    #[test]
    fn live_barrier_does_not_block_on_disconnected_editor() {
        // A disconnected editor must not deadlock the live finalize barrier.
        let (_dir, doc) = temp_doc("live-disconnect.md");
        let live = mint_client_id("vscode:live");
        let slow = mint_client_id("intellij:slow");
        with_hub(&doc, |hub| {
            hub.register(live).unwrap();
            hub.register(slow).unwrap();
            hub.local_edit(live, 0, 0, "LIVE").unwrap();
            hub.local_edit(slow, 0, 0, "SLOW").unwrap();
            hub.disconnect(slow);
        })
        .unwrap();
        assert!(commit_barrier_for_file_with_authority(
            &doc,
            CrdtAuthority::MultiReplica
        ));
        with_hub(&doc, |hub| {
            let cut = hub.canonical_text();
            assert!(cut.contains("LIVE"));
            assert!(
                !cut.contains("SLOW"),
                "disconnected op excluded, no deadlock"
            );
        })
        .unwrap();
    }

    #[test]
    fn live_supervisor_restart_recovers_canonical_from_ledger_projection() {
        // Supervisor restart: the live recovery path rebuilds the per-document
        // canonical replica from the last durable recovery projection.
        let (_dir, doc) = temp_doc("live-recover.md");
        let mut prior = RelayHub::new(1);
        let ed = mint_client_id("intellij:prior-restart");
        prior.register(ed).unwrap();
        prior.apply_local(ed, 0, 0, "survives-restart").unwrap();
        let projection = prior.projection_bytes();

        recover_hub_from_projection(&doc, &projection, None).unwrap();
        with_hub(&doc, |hub| {
            assert_eq!(hub.canonical_text(), "survives-restart");
        })
        .unwrap();
    }
}
