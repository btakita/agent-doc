//! Relay-hub SimWorld for editor attach: the JetBrains registration sequence
//! (`CrdtReplicaManager.forwarderFor`) driven against the real per-document
//! relay hub, so every re-registration meets the canonical, the coverage proof,
//! and the retained-projection answer the controller really gives.
//!
//! [`SimPluginEditor`] simulates only what lives in the IDE: the operator's
//! buffer, the settled shadow, the native replica, the retained native resume
//! state a library reload captures, and the order in which `forwarderFor`
//! registers, decides, checks the swap, and commits. Everything else is
//! production: registration (through the controller RPC's entry point), the
//! retained reseed, the live relay with its structure guard and origin fence,
//! the durable document-op ingest with its structural quarantine, the durable
//! queue-attempt projection, the native `registration_projection` derivation,
//! and the registration containment proof and clean merge the plugin asks
//! native for (`registration_contained`, `registration_rebase`).
//!
//! Found by the ambiguous-retained-projection attach fix (f155fe0076): each
//! scenario below is one of its live 2026-10-26 sequences on agent-doc-bugs.md.

use super::*;
use agent_doc_editor_surface::{
    RegistrationProjectionAction, RegistrationProjectionEvidence, derive_registration_projection,
};
use agent_doc_merge::captured_splice_request::{registration_contained, registration_rebase};
use agent_doc_merge::crdt_sync::ReplicaState;

const HOLD: &str = "ambiguous-retained-projection";

/// When a registration releases the retained native resume state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResumeRelease {
    /// `#resumefrontierlostonfailedattach` (f155fe0076): only once the
    /// registration commits (`releaseRetainedResumeStateOnCommitUtil`).
    OnCommit,
    /// The plugin before f155fe0076: after the projection decision and before
    /// the swap check, so an attempt the swap check refused had already lost it.
    BeforeSwap,
}

/// One registered native endpoint (`CrdtReplicaForwarder`).
struct SimForwarder {
    identity: String,
    replica: ReplicaState,
}

/// What one `forwarderFor` attempt did.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Attempt {
    Committed(RegistrationProjectionAction),
    Held(String),
    SwapRefused,
}

/// The facts one attempt decided from.
#[derive(Debug, Clone)]
struct AttemptFacts {
    sent_state_vector: bool,
    canonical_projection_retained: bool,
    retained_replica_reseed_pending: bool,
    canonical_covers_retained_frontier: Option<bool>,
    shadow: Option<String>,
    buffer: String,
    canonical: String,
    evidence: RegistrationProjectionEvidence,
    /// The ancestor the facts were proven from (`RegistrationCausalProof.mergeBase`).
    merge_base: Option<String>,
}

struct SimPluginEditor {
    doc: std::path::PathBuf,
    process: TestEditorProcess,
    base_identity: String,
    generation: u32,
    buffer: String,
    /// `settledShadows[path]` / `nativeReloadSettledShadows[path]`: the last
    /// text the editor proved the controller accepted.
    settled_shadow: Option<String>,
    /// `nativeReloadResumeStates[path]`: the encoded native replica a library
    /// reload captured.
    reload_resume_state: Option<Vec<u8>>,
    cached: Option<SimForwarder>,
    release: ResumeRelease,
    /// The controller holds a durable retained visible write for this
    /// document that the editor has not observed yet
    /// (`retained_write_observation_basis`), so its `replica_register` RPC
    /// forces a full canonical bootstrap.
    controller_retains_visible_write: bool,
    /// `#ackedreplicabase`: prove containment and the clean merge from the
    /// cached replica's text when canonical covers its frontier
    /// (`acknowledgedReplicaBaseUtil`). `false` is the plugin before it.
    acknowledged_base: bool,
    last: Option<AttemptFacts>,
}

impl SimPluginEditor {
    /// Open the document: the first registration bootstraps from canonical,
    /// which the buffer shows and the editor settles as its shadow.
    fn open(doc: &std::path::Path, release: ResumeRelease) -> Self {
        let process = TestEditorProcess::spawn();
        seed_live_reliable_sync_open_for_pid(&doc.display().to_string(), process.pid());
        let base_identity = format!("jetbrains-{}-simattach:{}", process.pid(), doc.display());
        let mut editor = Self {
            doc: doc.to_path_buf(),
            process,
            base_identity: base_identity.clone(),
            generation: 0,
            buffer: String::new(),
            settled_shadow: None,
            reload_resume_state: None,
            cached: None,
            release,
            controller_retains_visible_write: false,
            acknowledged_base: true,
            last: None,
        };
        let registration = editor
            .register_endpoint(&base_identity, None)
            .expect("initial editor registration");
        let replica =
            ReplicaState::from_encoded(registration.client_id, &registration.bootstrap).unwrap();
        editor.buffer = replica.text();
        editor.settled_shadow = Some(editor.buffer.clone());
        editor.cached = Some(SimForwarder {
            identity: base_identity,
            replica,
        });
        editor.settle_receipts();
        editor
    }

    /// The controller's `replica_register` RPC (`controller_crdt_replica_data`):
    /// a retained visible write forces a full canonical bootstrap, and the
    /// editor's retained frontier is passed through either way.
    fn register_endpoint(
        &self,
        identity: &str,
        retained_state_vector: Option<&[u8]>,
    ) -> Option<ReplicaRegistration> {
        register_replica_for_file_with_precondition(
            &self.doc,
            identity,
            retained_state_vector,
            Some(self.process.pid()),
            self.controller_retains_visible_write,
            None,
        )
        .unwrap()
    }

    fn forwarder(&self) -> &SimForwarder {
        self.cached.as_ref().expect("an attached forwarder")
    }

    /// Drain the committed endpoint's deliveries and acknowledge each visible
    /// projection, as the plugin does once a projection is applied.
    fn settle_receipts(&mut self) {
        let identity = self.forwarder().identity.clone();
        let Some(pull) = pull_replica_updates_for_file(&self.doc, &identity).unwrap() else {
            return;
        };
        for update in pull.updates {
            self.forwarder()
                .replica
                .apply_update(&update.update)
                .unwrap();
            let text = self.forwarder().replica.text();
            if agent_doc_hash::content_hash(&text) == update.expected_content_hash {
                observe_replica_projection_for_file(
                    &self.doc,
                    &identity,
                    &update.expected_content_hash,
                )
                .unwrap();
                self.buffer = text.clone();
                self.settled_shadow = Some(text);
            }
        }
    }

    /// The operator edits the buffer to `to`: the committed native replica
    /// follows. Returns the CRDT delta for the caller to route to a transport
    /// (local typing never advances the settled shadow).
    fn type_text(&mut self, to: &str) -> Vec<u8> {
        let delta = {
            let replica = &self.forwarder().replica;
            let frontier = replica.state_vector();
            apply_text_change(replica, &self.buffer, to);
            replica.diff(&frontier).unwrap()
        };
        self.buffer = to.to_string();
        delta
    }

    /// The operator edits the buffer to `to`, and the captured burst has not
    /// reached the native replica yet (`TypingTracker` drains it at the next
    /// quiet boundary), so neither the replica nor the controller has it.
    fn type_unapplied(&mut self, to: &str) {
        self.buffer = to.to_string();
    }

    /// `make install`: the native library reloads. The plugin captures the
    /// committed replica as the retained resume state, and the manager and its
    /// forwarders are recreated, so nothing is cached any more.
    fn reload_native_library(&mut self) {
        let forwarder = self.cached.take().expect("an attached forwarder");
        self.reload_resume_state = Some(forwarder.replica.encode_state());
    }

    /// One `forwarderFor(replaceCached = true)` attempt. `swap_moves` makes the
    /// swap check fail: the operator typed during registration, or a local
    /// delta is pending.
    fn reregister(&mut self, swap_moves: bool) -> Attempt {
        self.generation += 1;
        let identity = format!("{}:refresh-{}", self.base_identity, self.generation);
        // `#ackedreplicabase`: the cached replica's text beside the state the
        // registration sends (the sim's replica cannot move during the capture).
        let cached_text = self.cached.as_ref().map(|cached| cached.replica.text());
        let retained_resume_state = self
            .cached
            .as_ref()
            .map(|cached| cached.replica.encode_state())
            .or_else(|| self.reload_resume_state.clone());
        let retained_state_vector = retained_resume_state
            .as_ref()
            .map(|state| ReplicaState::from_encoded(1, state).unwrap().state_vector());
        let registration = self
            .register_endpoint(&identity, retained_state_vector.as_deref())
            .expect("the live editor registers");
        // `CrdtReplicaForwarder.register`: a delta bootstrap opens the retained
        // state, applies the delta, and publishes the retained suffix.
        let replica = if registration.incremental {
            let replica = ReplicaState::from_encoded(
                registration.client_id,
                retained_resume_state.as_ref().unwrap(),
            )
            .unwrap();
            if !registration.bootstrap.is_empty() {
                replica.apply_update(&registration.bootstrap).unwrap();
            }
            let suffix = replica.diff(&registration.canonical_state_vector).unwrap();
            relay_replica_update_for_file(&self.doc, &identity, &suffix)
                .unwrap()
                .expect("the retained suffix reaches the controller");
            replica
        } else {
            ReplicaState::from_encoded(registration.client_id, &registration.bootstrap).unwrap()
        };
        let canonical = replica.text();
        let shadow = self.settled_shadow.clone();
        let acknowledged = cached_text.filter(|_| {
            self.acknowledged_base && registration.canonical_covers_retained_frontier == Some(true)
        });
        let (evidence, merge_base) = registration_evidence(
            &registration,
            shadow.as_deref(),
            &self.buffer,
            &canonical,
            acknowledged.as_deref(),
        );
        let decision = derive_registration_projection(&evidence);
        self.last = Some(AttemptFacts {
            sent_state_vector: retained_state_vector.is_some(),
            canonical_projection_retained: registration.canonical_projection_retained,
            retained_replica_reseed_pending: registration.retained_replica_reseed_pending,
            canonical_covers_retained_frontier: registration.canonical_covers_retained_frontier,
            shadow: shadow.clone(),
            buffer: self.buffer.clone(),
            canonical: canonical.clone(),
            evidence,
            merge_base: merge_base.clone(),
        });
        if decision.action == RegistrationProjectionAction::HoldOperatorBuffer {
            deregister_replica_for_file(&self.doc, &identity).unwrap();
            return Attempt::Held(decision.hold_reason.unwrap());
        }
        if self.release == ResumeRelease::BeforeSwap && retained_resume_state.is_some() {
            self.reload_resume_state = None;
        }
        if swap_moves {
            deregister_replica_for_file(&self.doc, &identity).unwrap();
            return Attempt::SwapRefused;
        }
        // Commit: the endpoint owns the slot and its projection finalizes.
        let target = match decision.action {
            RegistrationProjectionAction::ApplyCanonical
            | RegistrationProjectionAction::DeferCanonicalProjection => canonical.clone(),
            RegistrationProjectionAction::PublishOperatorBuffer => self.buffer.clone(),
            // Recomputed from exactly the captured generations.
            RegistrationProjectionAction::MergeForward => {
                merge_from_base(merge_base.as_deref().unwrap(), &self.buffer, &canonical)
                    .expect("the clean merge recomputes from its proven ancestor")
            }
            RegistrationProjectionAction::HoldOperatorBuffer => unreachable!(),
        };
        if target != canonical {
            // `ensureEditorText`: publish the chosen text through the endpoint.
            let frontier = replica.state_vector();
            apply_text_change(&replica, &canonical, &target);
            let delta = replica.diff(&frontier).unwrap();
            relay_replica_update_for_file(&self.doc, &identity, &delta)
                .unwrap()
                .expect("the committed endpoint publishes");
        }
        if let Some(previous) = self.cached.take() {
            deregister_replica_for_file(&self.doc, &previous.identity).unwrap();
        }
        self.cached = Some(SimForwarder { identity, replica });
        self.buffer = target.clone();
        self.settled_shadow = Some(target);
        if self.release == ResumeRelease::OnCommit {
            self.reload_resume_state = None;
        }
        self.settle_receipts();
        self.controller_retains_visible_write = false;
        Attempt::Committed(decision.action)
    }

    fn last(&self) -> &AttemptFacts {
        self.last.as_ref().expect("an attempt")
    }
}

/// The facts `RegistrationProjection.evidence` reports, with the containment
/// proof (`canonicalContainsOperatorEditsAtRegistration`) and the clean merge
/// (`RegistrationSpliceShaping.cleanMerge`) answered by the native calls the
/// plugin makes, under the plugin's own guards: neither runs unless all three
/// generations are present and distinct.
///
/// `#ackedreplicabase` (`registrationCausalProofUtil`): when the settled shadow
/// proves neither, the same two calls run from `acknowledged`, the cached
/// replica text whose frontier canonical covers. Returns the facts and the
/// ancestor a merge-forward recomputes from.
fn registration_evidence(
    registration: &ReplicaRegistration,
    shadow: Option<&str>,
    buffer: &str,
    canonical: &str,
    acknowledged: Option<&str>,
) -> (RegistrationProjectionEvidence, Option<String>) {
    let distinct =
        shadow.filter(|shadow| buffer != *shadow && canonical != buffer && canonical != *shadow);
    let shadow_contained = distinct.map(|shadow| {
        registration_contained(shadow, buffer, canonical, true)
            .expect("the containment proof answers")
    });
    let shadow_merge =
        distinct.is_some_and(|shadow| registration_rebase(shadow, buffer, canonical).is_ok());
    let mut contained = shadow_contained;
    let mut clean_merge = shadow_merge;
    let mut merge_base = shadow.map(str::to_string);
    if shadow_contained != Some(true)
        && !shadow_merge
        && let Some(base) = acknowledged.filter(|base| Some(*base) != shadow && buffer != canonical)
    {
        if buffer == base {
            contained = Some(true);
            merge_base = Some(base.to_string());
        } else {
            let ack_contained = canonical != base
                && registration_contained(base, buffer, canonical, true)
                    .expect("the containment proof answers");
            let ack_merge = merge_from_base(base, buffer, canonical).is_some();
            if ack_contained || ack_merge {
                contained = Some(ack_contained);
                clean_merge = ack_merge;
                merge_base = Some(base.to_string());
            }
        }
    }
    let evidence = RegistrationProjectionEvidence {
        defer_canonical_projection_for_pending_local: false,
        canonical_projection_retained: registration.canonical_projection_retained,
        retained_replica_reseed_pending: registration.retained_replica_reseed_pending,
        canonical_covers_retained_frontier: registration.canonical_covers_retained_frontier,
        has_published_shadow: shadow.is_some(),
        has_buffer: true,
        has_canonical: true,
        buffer_equals_canonical: buffer == canonical,
        buffer_equals_shadow: shadow == Some(buffer),
        canonical_equals_shadow: shadow == Some(canonical),
        canonical_contains_operator_edits: contained,
        clean_merge_available: Some(clean_merge),
    };
    (evidence, merge_base)
}

/// `registrationMergeFromBaseUtil`: the buffer itself when canonical has not
/// moved past `base`, else the native rebase from `base`.
fn merge_from_base(base: &str, buffer: &str, canonical: &str) -> Option<String> {
    if buffer == canonical || buffer == base {
        return None;
    }
    if canonical == base {
        return Some(buffer.to_string());
    }
    registration_rebase(base, buffer, canonical)
        .ok()
        .map(|batch| batch.resulting_text)
}

fn hub_text(doc: &std::path::Path) -> String {
    with_hub(doc, |hub| hub.canonical_text()).unwrap()
}

const CLAIMED_ITEM: &str = "fix the attach hold";

/// `#resumefrontierlostonfailedattach` (live 2026-10-26 14:36-14:59): a
/// `make install` reloads the native library and recycles the controller. The
/// first re-registration reseeds the fresh controller from the retained replica
/// (and the controller projects its durable queue claim onto the result), then
/// the swap check refuses it. Returns the editor ready for its recovery probes
/// and the project directory that owns the document.
fn library_reload_then_refused_swap(
    release: ResumeRelease,
) -> (tempfile::TempDir, std::path::PathBuf, SimPluginEditor) {
    let (dir, doc) = temp_doc("reload-refused-swap.md");
    std::fs::write(
        &doc,
        format!(
            "<!-- agent:queue -->\n- {CLAIMED_ITEM}\n- unrelated pending work\n<!-- /agent:queue -->\n"
        ),
    )
    .unwrap();
    let mut editor = SimPluginEditor::open(&doc, release);

    editor.reload_native_library();
    forget_controller_relay_state_for_test(&doc);
    // The controller claimed a queue row. Its durable attempt is projected onto
    // the reseeded canonical, a visible write the editor has not observed.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut ledger = agent_doc_queue::queue_claim::QueueClaimLedger::default();
    ledger.claim(CLAIMED_ITEM, "subagent:attachhold", now, 3600);
    persist_queue_claim_ledger(dir.path(), &doc, &ledger);
    editor.controller_retains_visible_write = true;

    assert_eq!(editor.reregister(true), Attempt::SwapRefused);
    let reseed = editor.last();
    assert!(reseed.sent_state_vector);
    assert!(
        reseed.retained_replica_reseed_pending,
        "the first registration reseeds the fresh controller: {reseed:?}"
    );
    assert!(
        hub_text(&doc).contains(&format!("🟡 [#subagent:attachhold#1] {CLAIMED_ITEM}")),
        "the fresh controller projects its durable claim onto the reseed:\n{}",
        hub_text(&doc)
    );
    (dir, doc, editor)
}

/// `#resumefrontierlostonfailedattach`: an attempt the swap check refused keeps
/// the retained frontier, so the next registration still proves the controller
/// canonical covers it, and a buffer still equal to its settled shadow adopts
/// canonical (the claim projection) instead of holding.
#[test]
fn library_reload_then_refused_swap_reregisters_with_the_retained_frontier() {
    let (_dir, doc, mut editor) = library_reload_then_refused_swap(ResumeRelease::OnCommit);
    let operator_text = editor.buffer.clone();

    let recovery = editor.reregister(false);
    let facts = editor.last().clone();
    assert!(
        facts.sent_state_vector,
        "the refused attempt kept the retained frontier"
    );
    assert!(facts.canonical_projection_retained, "{facts:?}");
    assert_eq!(facts.canonical_covers_retained_frontier, Some(true));
    assert_eq!(
        facts.shadow.as_deref(),
        Some(facts.buffer.as_str()),
        "the live signature: shadow_hash == buffer_hash"
    );
    assert_ne!(facts.canonical, facts.buffer);
    assert_eq!(
        recovery,
        Attempt::Committed(RegistrationProjectionAction::ApplyCanonical),
        "{facts:#?}"
    );

    // The editor converged on canonical: the claim marker shows, every
    // operator row is intact, and nothing is left to deliver.
    assert_eq!(editor.buffer, hub_text(&doc));
    assert_eq!(
        editor.buffer,
        operator_text.replacen(
            &format!("- 📥 {CLAIMED_ITEM}"),
            &format!("- 🟡 [#subagent:attachhold#1] {CLAIMED_ITEM}"),
            1,
        ),
    );
    assert_eq!(
        editor.settled_shadow.as_deref(),
        Some(editor.buffer.as_str())
    );
    assert!(editor.reload_resume_state.is_none(), "released on commit");
    with_hub(&doc, |hub| assert!(hub.delivery_converged())).unwrap();
}

/// The control for the test above: the plugin's pre-f155fe0076 release order
/// reproduces the live hold. The refused attempt had released the retained
/// frontier, so every recovery probe sends no state vector, coverage is
/// unknown, and the same hub holds `ambiguous-retained-projection` forever over
/// a buffer that still equals its settled shadow.
#[test]
fn a_frontier_released_before_the_swap_check_holds_every_recovery_probe() {
    let (_dir, doc, mut editor) = library_reload_then_refused_swap(ResumeRelease::BeforeSwap);
    let operator_text = editor.buffer.clone();
    for probe in 0..3 {
        assert_eq!(
            editor.reregister(false),
            Attempt::Held(HOLD.to_string()),
            "probe {probe}: {:#?}",
            editor.last()
        );
        let facts = editor.last();
        assert!(!facts.sent_state_vector);
        assert!(facts.canonical_projection_retained);
        assert_eq!(facts.canonical_covers_retained_frontier, None);
        assert_eq!(facts.shadow.as_deref(), Some(facts.buffer.as_str()));
        assert_eq!(
            editor.buffer, operator_text,
            "a hold never touches the buffer"
        );
    }
    assert!(hub_text(&doc).contains("🟡 [#subagent:attachhold#1]"));
}

const LOOP_CLOSE: &str = "<!-- /agent:loop -->\n";

/// `#lagcanonicalrollforward`, structure fence (live 2026-10-26 13:48-13:53):
/// the operator types a row into the loop (`- typed`), then deletes the loop's
/// close marker and keeps typing on that row (`- typed more`). The durable
/// document-op plane ingests the first frame. The second would break the
/// component structure, so it is quarantined, which fences its live author;
/// the same frame then arrives on the live relay and is refused. Canonical
/// holds a prefix of the operator's text and the settled shadow never moved.
/// Returns the editor ready to re-register.
fn structure_quarantine_with_durable_ingest()
-> (tempfile::TempDir, std::path::PathBuf, SimPluginEditor) {
    let (dir, doc) = temp_doc("structure-quarantine-lag.md");
    std::fs::write(
        &doc,
        format!("# Lag\n\n<!-- agent:loop -->\nwatch row\n{LOOP_CLOSE}## Todo\nb\n"),
    )
    .unwrap();
    let mut editor = SimPluginEditor::open(&doc, ResumeRelease::OnCommit);
    let settled = editor.buffer.clone();
    let lineage = current_lineage_for_file(&doc).unwrap().expect("lineage");
    let identity = editor.forwarder().identity.clone();

    let typed = settled.replacen(LOOP_CLOSE, &format!("- typed\n{LOOP_CLOSE}"), 1);
    let first = editor.type_text(&typed);
    assert_eq!(
        apply_document_op_delta_for_file_in_lineage(&doc, Some(&lineage), &first).unwrap(),
        Some(DocumentOpDeltaOutcome::Applied { changed: true }),
    );
    relay_replica_update_for_file(&doc, &identity, &first)
        .unwrap()
        .expect("the live relay replays the ingested frame");

    let more = settled.replacen(LOOP_CLOSE, "- typed more\n", 1);
    let second = editor.type_text(&more);
    assert_eq!(
        apply_document_op_delta_for_file_in_lineage(&doc, Some(&lineage), &second).unwrap(),
        Some(DocumentOpDeltaOutcome::StructuralQuarantined),
    );
    let refused = relay_replica_update_for_file(&doc, &identity, &second)
        .unwrap()
        .expect("the fenced live update is terminally handled");
    assert!(refused.update.is_empty() && refused.targets.is_empty());

    assert_eq!(hub_text(&doc), typed, "canonical lags the buffer");
    assert_eq!(editor.buffer, more);
    assert_eq!(editor.settled_shadow.as_deref(), Some(settled.as_str()));
    (dir, doc, editor)
}

/// `#lagcanonicalrollforward`: the re-registration meets three distinct
/// generations. Canonical does not contain the buffer (the later typing is
/// missing) and the line merge refuses, but the buffer carries every change
/// canonical made since the shadow, so the clean merge answers the buffer
/// itself and registration merges it forward instead of holding. The editor
/// text is the base: nothing rolls back toward the stale shadow, the ingested
/// typing is not inserted twice, and the broken structure stays fenced (never
/// committed) until the operator repairs it.
#[test]
fn structure_quarantined_typing_with_durable_ingest_merges_the_buffer_forward() {
    let (_dir, doc, mut editor) = structure_quarantine_with_durable_ingest();
    let operator_text = editor.buffer.clone();

    let attempt = editor.reregister(false);
    let facts = editor.last().clone();
    assert!(facts.canonical_projection_retained, "{facts:?}");
    assert_eq!(facts.canonical_covers_retained_frontier, Some(false));
    assert_eq!(
        facts.evidence.canonical_contains_operator_edits,
        Some(false)
    );
    assert_eq!(
        facts.evidence.clean_merge_available,
        Some(true),
        "{facts:#?}"
    );
    assert_eq!(
        attempt,
        Attempt::Committed(RegistrationProjectionAction::MergeForward),
        "{facts:#?}"
    );

    assert_eq!(editor.buffer, operator_text, "editor text is the base");
    let canonical = hub_text(&doc);
    assert_eq!(
        canonical, operator_text,
        "registration published the buffer"
    );
    assert_eq!(
        canonical.matches("- typed").count(),
        1,
        "never inserted twice"
    );
    assert!(
        !canonical.contains(LOOP_CLOSE),
        "the operator's deletion stands"
    );
    with_hub(&doc, |hub| {
        assert!(
            hub.component_structure_refusal().is_some(),
            "an unpaired loop is never committable canonical"
        );
    })
    .unwrap();

    // The operator restores the close marker: the fence releases on that op.
    let repaired =
        operator_text.replacen("- typed more\n", &format!("- typed more\n{LOOP_CLOSE}"), 1);
    let repair = editor.type_text(&repaired);
    relay_replica_update_for_file(&doc, &editor.forwarder().identity, &repair)
        .unwrap()
        .expect("the repair relays");
    assert_eq!(hub_text(&doc), repaired);
    with_hub(&doc, |hub| {
        assert!(hub.component_structure_refusal().is_none())
    })
    .unwrap();
}

/// The bound on the roll-forward: the same lagging sequence, plus a controller
/// write the buffer does not carry (a response row under `## Todo`), still
/// holds. Neither direction is proven lossless, and the hold leaves both the
/// buffer and the controller's row exactly as they were.
#[test]
fn a_lagging_canonical_with_a_controller_write_the_buffer_lacks_still_holds() {
    let (_dir, doc, mut editor) = structure_quarantine_with_durable_ingest();
    let operator_text = editor.buffer.clone();
    let lagging = hub_text(&doc);
    let responded = lagging.replacen("## Todo\n", "## Todo\n### Re: response\n", 1);
    apply_cp_write_for_file(&doc, &lagging, &responded, "test_lagging_controller_write")
        .unwrap()
        .expect("the controller write uses the live relay");

    let attempt = editor.reregister(false);
    let facts = editor.last().clone();
    assert_eq!(facts.canonical, responded, "{facts:#?}");
    assert_eq!(attempt, Attempt::Held(HOLD.to_string()), "{facts:#?}");
    assert_eq!(editor.buffer, operator_text);
    assert_eq!(hub_text(&doc), responded);
}

const RUN_ROW: &str = "- operator row\n";
const RUN_TYPED: &str = "- operator row the operator typed\n";
const RUN_RETYPED: &str = "- operator row the operator typod\n";

/// `#ackedreplicabase` (live agent-doc-bugs.md, 2026-10-28 12:57:15-12:57:53):
/// the operator types a row without pause. Every burst reaches the controller
/// through the cached replica, but no visible projection settles, so the
/// settled shadow stays at the text before the typing. The resume-id capture
/// writes the frontmatter (undelivered: the editor is mid-typing), and the
/// operator retypes one character the replica has not applied yet. The
/// re-registration then meets three distinct generations whose stale common
/// ancestor makes canonical's copy of the typing and the buffer's copy two
/// changes of one line. Returns the editor ready to re-register.
fn continuous_typing_beside_an_undelivered_frontmatter_write(
    acknowledged_base: bool,
) -> (tempfile::TempDir, std::path::PathBuf, SimPluginEditor) {
    let (dir, doc) = temp_doc("ackedreplicabase.md");
    std::fs::write(
        &doc,
        format!(
            "---\nresume:\n  claude: old-conversation\npresets:\n  '#repo': 'https://github.com/btakita/agent-doc'\n---\n\n## Notes\n\n{RUN_ROW}\n## Todo\nb\n"
        ),
    )
    .unwrap();
    let mut editor = SimPluginEditor::open(&doc, ResumeRelease::OnCommit);
    editor.acknowledged_base = acknowledged_base;
    let settled = editor.buffer.clone();
    let identity = editor.forwarder().identity.clone();

    let typed = settled.replacen(RUN_ROW, RUN_TYPED, 1);
    let burst = editor.type_text(&typed);
    relay_replica_update_for_file(&doc, &identity, &burst)
        .unwrap()
        .expect("the typing reaches the controller");
    assert_eq!(hub_text(&doc), typed);

    let resumed = typed.replacen("claude: old-conversation", "claude: new-conversation", 1);
    apply_cp_write_for_file(&doc, &typed, &resumed, "start_resume_id_capture")
        .unwrap()
        .expect("the controller write uses the live relay");
    editor.controller_retains_visible_write = true;

    editor.type_unapplied(&typed.replacen(RUN_TYPED, RUN_RETYPED, 1));
    assert_eq!(
        editor.settled_shadow.as_deref(),
        Some(settled.as_str()),
        "no projection settled while the operator typed"
    );
    (dir, doc, editor)
}

/// `#ackedreplicabase`: the cached replica's text is a proven ancestor
/// (canonical covers its frontier), and from it the retype and the resume-id
/// write touch different lines. Registration merges the buffer forward: the
/// operator's retype and the controller's write both survive, nothing is
/// inserted twice, and the operator's quoting is untouched.
#[test]
fn continuous_typing_beside_an_undelivered_controller_write_merges_from_the_acknowledged_replica() {
    let (_dir, doc, mut editor) = continuous_typing_beside_an_undelivered_frontmatter_write(true);
    let operator_text = editor.buffer.clone();

    let attempt = editor.reregister(false);
    let facts = editor.last().clone();
    assert!(facts.canonical_projection_retained, "{facts:?}");
    assert_eq!(facts.canonical_covers_retained_frontier, Some(true));
    assert_ne!(
        facts.shadow, facts.merge_base,
        "proven from the acknowledged text"
    );
    assert_eq!(
        facts.evidence.clean_merge_available,
        Some(true),
        "{facts:#?}"
    );
    assert_eq!(
        attempt,
        Attempt::Committed(RegistrationProjectionAction::MergeForward),
        "{facts:#?}"
    );

    let expected =
        operator_text.replacen("claude: old-conversation", "claude: new-conversation", 1);
    assert_eq!(editor.buffer, expected);
    assert_eq!(hub_text(&doc), expected);
    assert_eq!(expected.matches("the operator typ").count(), 1);
    assert!(expected.contains("  '#repo': 'https://github.com/btakita/agent-doc'\n"));
}

/// The control: proven from the stale settled shadow alone (the plugin before
/// `#ackedreplicabase`), the same sequence holds `ambiguous-retained-projection`
/// -- the live 12:57:53 refusal -- and the hold leaves the buffer untouched.
#[test]
fn continuous_typing_beside_an_undelivered_controller_write_holds_from_the_stale_shadow() {
    let (_dir, doc, mut editor) = continuous_typing_beside_an_undelivered_frontmatter_write(false);
    let operator_text = editor.buffer.clone();
    let canonical = hub_text(&doc);

    assert_eq!(
        editor.reregister(false),
        Attempt::Held(HOLD.to_string()),
        "{:#?}",
        editor.last()
    );
    let facts = editor.last();
    assert_eq!(facts.canonical_covers_retained_frontier, Some(true));
    assert_eq!(
        facts.evidence.canonical_contains_operator_edits,
        Some(false)
    );
    assert_eq!(facts.evidence.clean_merge_available, Some(false));
    assert_eq!(editor.buffer, operator_text);
    assert_eq!(hub_text(&doc), canonical);
}

/// The bound: from the acknowledged text, a controller rewrite of the very row
/// the operator is retyping is still a conflict, so registration holds and
/// neither side's text moves.
#[test]
fn a_controller_rewrite_of_the_retyped_row_still_holds_from_the_acknowledged_replica() {
    let (_dir, doc, mut editor) = continuous_typing_beside_an_undelivered_frontmatter_write(true);
    let operator_text = editor.buffer.clone();
    let current = hub_text(&doc);
    let rewritten = current.replacen(RUN_TYPED, "- operator row rewritten by the controller\n", 1);
    apply_cp_write_for_file(&doc, &current, &rewritten, "test_controller_row_rewrite")
        .unwrap()
        .expect("the controller write uses the live relay");

    let attempt = editor.reregister(false);
    let facts = editor.last().clone();
    assert_eq!(facts.canonical_covers_retained_frontier, Some(true));
    assert_eq!(attempt, Attempt::Held(HOLD.to_string()), "{facts:#?}");
    assert_eq!(editor.buffer, operator_text);
    assert_eq!(hub_text(&doc), rewritten);
}
