//! Cross-process editor liveness as OR-set / LWW cells (`#lzsync-liveness`,
//! sidecar-retirement Phase 3C — the controller *receiver* core).
//!
//! This is the sole editor-liveness authority: the controller folds liveness
//! frames pushed by the editor plugins into Lazily's proven convergent cells and
//! derives the open-set / per-pid-alive / live-doc aggregate.
//!
//! The semantics are exactly the ones lazily-spec pins and lazily-formal proves
//! (`ReliableSync.crdt_liveness_convergence_under_retry`,
//! `orset_add_wins_over_stale_remove`, the `joinReg_*` semilattice), so this
//! module uses lazily's [`OrSet`] and [`WireLwwRegister`] directly rather than a
//! bespoke re-implementation:
//!
//! - **Open-set membership** — "editor pid P has doc D open" — is an observed-remove
//!   set ([`OrSet`]): a re-open (fresh add tag) wins over a concurrent lagging close
//!   that only observed the earlier tag (add-wins).
//! - **Per-pid `alive`** — the OS process-exit watcher (S4b) writes `alive = false`
//!   at a fresh HLC stamp — is a [`WireLwwRegister`] (highest stamp wins), so a stale
//!   re-assert is dominated.
//! - **Derived "doc D is live"** = any *present* `(D, P)` whose `alive[P]` is true.
//!   One `alive[P] = false` fans out to every doc P held (whole-editor-death cascade),
//!   which falls out of the per-`(doc, pid)` iteration for free.
//!
//! Keys are namespaced by `document_hash` (the per-doc isolation invariant: a stale
//! overlay for doc B cannot flip doc A). The liveness ops ride the CrdtSync plane
//! (spec § `#lzsync-liveness`): [`encode_liveness_frame`] packs a `LivenessOp` batch
//! into one `IpcMessage::CrdtSync` op as inline bytes, so the batch flows through the
//! same reliable-sync `SyncDriver` + `DurableOutbox` as every other frame (the driver
//! hands an applied `CrdtSync` straight to the host, which folds it here).

use anyhow::{Result, anyhow};
use lazily::{CrdtOp, CrdtSync, IpcMessage, IpcValue, NodeId, OrSet, WireLwwRegister, WireStamp};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// OS process id of an editor.
pub type Pid = u64;

/// Package generation expected by this controller for an editor registration.
///
/// This is owned beside [`EditorRegistration`] because the registration is the
/// only generation-bearing authority for native editor effects. Unknown kinds
/// fail closed (`None`) instead of borrowing an unversioned replica identity.
pub fn expected_editor_plugin_version(editor_kind: &str) -> Option<&'static str> {
    match editor_kind.trim().to_ascii_lowercase().as_str() {
        "jetbrains" | "intellij" | "idea" | "jb" => {
            option_env!("AGENT_DOC_EXPECTED_JETBRAINS_PLUGIN_VERSION")
        }
        "vscode" | "vs-code" | "code" => {
            option_env!("AGENT_DOC_EXPECTED_VSCODE_PLUGIN_VERSION")
        }
        "zed" | "zed-lsp" => option_env!("AGENT_DOC_EXPECTED_ZED_PLUGIN_VERSION"),
        _ => None,
    }
}

/// Package generation expected for the proven JetBrains platform line.
///
/// JetBrains 262 and 263 use separately versioned packages. Callers that can
/// prove the live IDE build must compare against that package generation, not
/// the classic 242-261 generation. Unknown and unsupported builds retain the
/// classic fallback so the existing unsupported-build diagnostic can explain
/// that no matching package is published.
pub fn expected_editor_plugin_version_for_platform_build(
    editor_kind: &str,
    platform_build: Option<u32>,
) -> Option<&'static str> {
    if matches!(
        editor_kind.trim().to_ascii_lowercase().as_str(),
        "jetbrains" | "intellij" | "idea" | "jb"
    ) {
        return match platform_build {
            Some(262) => option_env!("AGENT_DOC_EXPECTED_JETBRAINS_262_PLUGIN_VERSION")
                .or_else(|| expected_editor_plugin_version(editor_kind)),
            Some(263) => option_env!("AGENT_DOC_EXPECTED_JETBRAINS_263_PLUGIN_VERSION")
                .or_else(|| expected_editor_plugin_version(editor_kind)),
            _ => expected_editor_plugin_version(editor_kind),
        };
    }
    expected_editor_plugin_version(editor_kind)
}

/// Exact generation identity with the editor kind's primary (classic) package.
///
/// This is a *display/staleness* question, not the native-save admission
/// question: see [`editor_native_save_compatibility`] (GH #236).
pub fn editor_plugin_generation_matches(editor_kind: &str, running: &str) -> bool {
    expected_editor_plugin_version(editor_kind).is_some_and(|expected| running.trim() == expected)
}

/// Every package version this binary's release pairs with an editor kind, one
/// per IDE line (GH #236). JetBrains ships a classic-source package for 242-261
/// plus modular 262 and 263 packages (GH #249) under three distinct versions.
pub fn release_paired_editor_plugin_versions(editor_kind: &str) -> Vec<&'static str> {
    let mut versions = Vec::new();
    if let Some(primary) = expected_editor_plugin_version(editor_kind) {
        versions.push(primary);
    }
    if matches!(
        editor_kind.trim().to_ascii_lowercase().as_str(),
        "jetbrains" | "intellij" | "idea" | "jb"
    ) && let Some(modular) = option_env!("AGENT_DOC_EXPECTED_JETBRAINS_262_PLUGIN_VERSION")
        && !versions.contains(&modular)
    {
        versions.push(modular);
    }
    if matches!(
        editor_kind.trim().to_ascii_lowercase().as_str(),
        "jetbrains" | "intellij" | "idea" | "jb"
    ) && let Some(modern) = option_env!("AGENT_DOC_EXPECTED_JETBRAINS_263_PLUGIN_VERSION")
        && !versions.contains(&modern)
    {
        versions.push(modern);
    }
    versions
}

/// Whether `running` is one of [`release_paired_editor_plugin_versions`].
pub fn editor_plugin_version_is_release_paired(editor_kind: &str, running: &str) -> bool {
    release_paired_editor_plugin_versions(editor_kind).contains(&running.trim())
}

/// Oldest plugin generation that speaks the native-save (`persist_current`)
/// protocol this binary requires, per editor kind (GH #236).
///
/// The protocol, not the package version, is what a disk-writing native save
/// depends on: a `persist_current` intent carrying the exact visible
/// `expected_content_hash`/`expected_content_len`, answered by a terminal
/// receipt (`applied`, `rejected`, or `deferred`). Floors:
///
/// - JetBrains `0.2.497` (`49241abcf`, netadv5 R2): the first build that
///   answers a slow document-lane save `deferred` instead of `rejected`. An
///   older build turns a timeout into a definitive refusal, which drops a live
///   editor from the delivery cut, so it is a real protocol gap. The Kotlin
///   `persist_current` path has not changed since.
/// - VS Code `0.2.67`: the first version string not shared with builds that
///   predate `persist_current` (`5c40923a4`; `0.2.66` shipped both sides).
///
/// The floor only moves when the binary starts requiring something new on this
/// path; a change to the intent's meaning must use a new intent name, so a
/// newer plugin than this binary knows still speaks the floor's protocol.
pub fn native_save_protocol_floor(editor_kind: &str) -> Option<&'static str> {
    match editor_kind.trim().to_ascii_lowercase().as_str() {
        "jetbrains" | "intellij" | "idea" | "jb" => Some("0.2.497"),
        "vscode" | "vs-code" | "code" => Some("0.2.67"),
        _ => None,
    }
}

/// Native-save admission verdict for one registration (GH #236).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeSaveCompatibility {
    /// The running plugin is a package this release ships (any IDE line).
    ReleasePaired,
    /// Not a release-paired package (an older package left on an IDE build no
    /// package covers, or a newer one), but at or above the protocol floor.
    ProtocolCompatible { floor: &'static str },
    /// Older than the protocol floor: a real protocol gap. Fail closed.
    BelowProtocolFloor { floor: &'static str },
    /// The running version does not parse, so no protocol can be claimed.
    UnparsableVersion { floor: &'static str },
    /// No native-save protocol is known for this editor kind.
    UnknownKind,
}

impl NativeSaveCompatibility {
    pub const fn admits_native_save(self) -> bool {
        matches!(self, Self::ReleasePaired | Self::ProtocolCompatible { .. })
    }

    /// Stable ops-log token.
    pub const fn token(self) -> &'static str {
        match self {
            Self::ReleasePaired => "release_paired",
            Self::ProtocolCompatible { .. } => "protocol_compatible",
            Self::BelowProtocolFloor { .. } => "below_native_save_protocol_floor",
            Self::UnparsableVersion { .. } => "unparsable_plugin_version",
            Self::UnknownKind => "unknown_editor_kind",
        }
    }

    pub const fn floor(self) -> Option<&'static str> {
        match self {
            Self::ProtocolCompatible { floor }
            | Self::BelowProtocolFloor { floor }
            | Self::UnparsableVersion { floor } => Some(floor),
            Self::ReleasePaired | Self::UnknownKind => None,
        }
    }
}

fn parse_dotted_version(version: &str) -> Option<Vec<u64>> {
    let core = version.trim().trim_start_matches('v');
    let core = core.split(['-', '+']).next().unwrap_or(core);
    if core.is_empty() {
        return None;
    }
    core.split('.')
        .map(|part| part.parse::<u64>().ok())
        .collect::<Option<Vec<_>>>()
}

fn dotted_version_at_least(running: &[u64], floor: &[u64]) -> bool {
    for index in 0..running.len().max(floor.len()) {
        let run = running.get(index).copied().unwrap_or(0);
        let min = floor.get(index).copied().unwrap_or(0);
        if run != min {
            return run > min;
        }
    }
    true
}

/// Whether a registration may receive a disk-writing native save (GH #236).
///
/// Admission is protocol compatibility, not equality with the classic package
/// version: the 262 and 263 ranged packages speak the same `persist_current`
/// protocol as the primary classic package, despite distinct update versions.
pub fn editor_native_save_compatibility(
    editor_kind: &str,
    running: &str,
) -> NativeSaveCompatibility {
    let Some(floor) = native_save_protocol_floor(editor_kind) else {
        return NativeSaveCompatibility::UnknownKind;
    };
    if editor_plugin_version_is_release_paired(editor_kind, running) {
        return NativeSaveCompatibility::ReleasePaired;
    }
    let (Some(run), Some(min)) = (parse_dotted_version(running), parse_dotted_version(floor))
    else {
        return NativeSaveCompatibility::UnparsableVersion { floor };
    };
    if dotted_version_at_least(&run, &min) {
        NativeSaveCompatibility::ProtocolCompatible { floor }
    } else {
        NativeSaveCompatibility::BelowProtocolFloor { floor }
    }
}

/// Metadata for one editor replica. This travels on the same reliable Lazily
/// channel as open/close state; it is not a filesystem projection of the live
/// buffer. Registrations are scoped by document and pid, and only registrations
/// whose pid is currently live/open are exposed as authority.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EditorRegistration {
    pub document_hash: String,
    pub pid: Pid,
    pub path: String,
    pub editor_id: String,
    pub editor_kind: String,
    pub editor_version: String,
    pub capabilities: Vec<String>,
    pub timestamp_ms: u64,
}

/// One liveness event pushed editor-plugin → controller on the reliable-sync plane.
///
/// The open/close pair carries the OR-set observation `tag`s (not stamps — OR-set
/// convergence is tag-set union); `Alive` carries the LWW `WireStamp`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LivenessOp {
    /// Editor `pid` opened / attached `document_hash`; `tag` is a fresh presence id
    /// minted by the plugin (a re-open mints a new tag → add-wins over a stale close).
    Open {
        document_hash: String,
        pid: Pid,
        tag: String,
    },
    /// Editor `pid` closed `document_hash`, observing `observed_tags` (only those
    /// add-tags are shadowed; a concurrent re-open's fresh tag survives).
    Close {
        document_hash: String,
        pid: Pid,
        observed_tags: Vec<String>,
    },
    /// Per-pid liveness write (the OS exit watcher sets `value = false` at a fresh
    /// stamp; the highest stamp wins).
    Alive {
        pid: Pid,
        value: bool,
        stamp: WireStamp,
    },
    /// Per-`(document_hash, pid)` sync-progress write: the editor's current
    /// `edit_epoch` (monotonic count of local edit batches) and `synced_epoch` (the
    /// highest edit batch the editor has confirmed the controller/CRDT received).
    /// `edit_epoch > synced_epoch` ⇒ the editor holds unsynced edits in flight. This
    /// is the plane replacement for the divergence signal the live-buffer sidecar's
    /// `edit_epoch`/`last_synced_epoch` fields carry (`editor_sync_statuses`); highest
    /// LWW `stamp` wins, so a later report supersedes and a re-delivery is a no-op
    /// (#sidecar-retirement sync-in-flight foundation).
    Sync {
        document_hash: String,
        pid: Pid,
        edit_epoch: u64,
        synced_epoch: u64,
        stamp: WireStamp,
    },
    /// Editor identity/generation/capabilities for one document replica. The
    /// value is folded monotonically by `(timestamp_ms, value)` so retry and
    /// reordering converge without a separate live-buffer metadata sidecar.
    Register(EditorRegistration),
}

/// The controller's convergent liveness projection over one or more documents.
///
/// Fold [`LivenessOp`]s in via [`apply`](Self::apply) (idempotent + order-independent —
/// re-delivery is a no-op), then read the derived authority: [`is_open`](Self::is_open),
/// [`pid_alive`](Self::pid_alive), [`open_docs`](Self::open_docs), [`live_docs`](Self::live_docs).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct LivenessProjection {
    /// `(document_hash, pid)` → observed-remove membership set.
    open_set: BTreeMap<(String, Pid), OrSet>,
    /// Number of currently-present document memberships per pid.
    ///
    /// The process-exit watcher polls the distinct open pids every 500 ms. Its
    /// candidate set changes only on an actual OR-set membership edge, so retain
    /// this derived index instead of rescanning every document and recomputing
    /// every tag-set difference on each idle poll. Applying the same fact again
    /// leaves both the OR-set and this count unchanged.
    open_pid_counts: BTreeMap<Pid, usize>,
    /// `pid` → LWW `alive` register.
    alive: BTreeMap<Pid, WireLwwRegister<bool>>,
    /// `(document_hash, pid)` → LWW `(edit_epoch, synced_epoch)` register. The
    /// plane's sync-in-flight signal (`edit_epoch > synced_epoch` ⇒ unsynced edits),
    /// replacing the live-buffer sidecar's epoch fields (#sidecar-retirement).
    sync_state: BTreeMap<(String, Pid), WireLwwRegister<(u64, u64)>>,
    /// `(document_hash, pid)` -> newest deterministic editor registration.
    registrations: BTreeMap<(String, Pid), EditorRegistration>,
}

impl LivenessProjection {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold one liveness op into the projection. Idempotent (OR-set add/remove are
    /// set unions; the LWW `set` only adopts a strictly-higher stamp), so a replay
    /// or re-delivery changes nothing.
    pub fn apply(&mut self, op: &LivenessOp) {
        match op {
            LivenessOp::Open {
                document_hash,
                pid,
                tag,
            } => {
                let (was_present, is_present) = {
                    let membership = self
                        .open_set
                        .entry((document_hash.clone(), *pid))
                        .or_default();
                    let was_present = membership.present();
                    membership.add(tag.clone());
                    (was_present, membership.present())
                };
                self.update_open_pid_count(*pid, was_present, is_present);
            }
            LivenessOp::Close {
                document_hash,
                pid,
                observed_tags,
            } => {
                // A close seen before its open still records the shadow so a late,
                // already-observed add cannot resurrect the entry.
                let (was_present, is_present) = {
                    let membership = self
                        .open_set
                        .entry((document_hash.clone(), *pid))
                        .or_default();
                    let was_present = membership.present();
                    membership.remove_observed(observed_tags.iter().cloned());
                    (was_present, membership.present())
                };
                self.update_open_pid_count(*pid, was_present, is_present);
            }
            LivenessOp::Alive { pid, value, stamp } => match self.alive.get_mut(pid) {
                Some(reg) => reg.set(*stamp, *value),
                None => {
                    self.alive
                        .insert(*pid, WireLwwRegister::new(*stamp, *value));
                }
            },
            LivenessOp::Sync {
                document_hash,
                pid,
                edit_epoch,
                synced_epoch,
                stamp,
            } => {
                let key = (document_hash.clone(), *pid);
                let value = (*edit_epoch, *synced_epoch);
                match self.sync_state.get_mut(&key) {
                    Some(reg) => reg.set(*stamp, value),
                    None => {
                        self.sync_state
                            .insert(key, WireLwwRegister::new(*stamp, value));
                    }
                }
            }
            LivenessOp::Register(registration) => {
                let key = (registration.document_hash.clone(), registration.pid);
                let replace = self.registrations.get(&key).is_none_or(|current| {
                    registration.timestamp_ms > current.timestamp_ms
                        || (registration.timestamp_ms == current.timestamp_ms
                            && registration > current)
                });
                if replace {
                    self.registrations.insert(key, registration.clone());
                }
            }
        }
    }

    /// Fold a whole batch (a decoded liveness frame).
    pub fn apply_batch(&mut self, ops: &[LivenessOp]) {
        for op in ops {
            self.apply(op);
        }
    }

    /// Apply one real `(document, pid)` membership edge to the derived pid
    /// index. Replays and stale observed-removes have `before == after` and are
    /// deliberately no-ops.
    fn update_open_pid_count(&mut self, pid: Pid, before: bool, after: bool) {
        match (before, after) {
            (false, true) => {
                let count = self.open_pid_counts.entry(pid).or_default();
                *count = count.checked_add(1).expect("open document count overflow");
            }
            (true, false) => {
                let remove = match self.open_pid_counts.get_mut(&pid) {
                    Some(count) if *count > 1 => {
                        *count -= 1;
                        false
                    }
                    Some(_) => true,
                    None => {
                        debug_assert!(false, "present open membership must have a pid count");
                        false
                    }
                };
                if remove {
                    self.open_pid_counts.remove(&pid);
                }
            }
            (false, false) | (true, true) => {}
        }
    }

    /// Is `(document_hash, pid)` currently open (some add-tag not shadowed)?
    pub fn is_open(&self, document_hash: &str, pid: Pid) -> bool {
        self.open_set
            .get(&(document_hash.to_string(), pid))
            .is_some_and(OrSet::present)
    }

    /// Is `pid` alive? Absent register ⇒ presumed alive (an open editor is live
    /// until a death signal arrives — the death is the LWW `alive = false`, not a
    /// missing entry).
    pub fn pid_alive(&self, pid: Pid) -> bool {
        self.alive.get(&pid).map(|r| *r.value()).unwrap_or(true)
    }

    /// Distinct pids that currently hold `document_hash` open (regardless of alive).
    pub fn open_pids(&self, document_hash: &str) -> BTreeSet<Pid> {
        self.open_set
            .iter()
            .filter(|((doc, _), set)| doc == document_hash && set.present())
            .map(|((_, pid), _)| *pid)
            .collect()
    }

    /// Every pid with at least one present open-set membership. Controller
    /// hydration uses this to reconcile durable open facts with OS process
    /// liveness after a crash/restart where the editor's exit watcher could not
    /// publish its terminal `Alive(false)` fact.
    pub fn all_open_pids(&self) -> BTreeSet<Pid> {
        self.open_pid_counts.keys().copied().collect()
    }

    /// Whether this projection has ever received an open/close fact for the
    /// document. Unlike `open_docs().is_empty()`, this distinguishes a durably
    /// known closed document from a never-hydrated cold process.
    pub fn tracks_document(&self, document_hash: &str) -> bool {
        self.open_set
            .range((document_hash.to_owned(), Pid::MIN)..=(document_hash.to_owned(), Pid::MAX))
            .next()
            .is_some()
    }

    /// Derived live-editor authority for one document.
    ///
    /// `None` is a true cold miss: this projection has never received an
    /// open/close fact for `document_hash`. `Some(false)` includes a tracked,
    /// known-closed document and a document held only by dead editor pids.
    /// Because `(document_hash, pid)` is the ordered open-set key, the lookup
    /// touches only this document's contiguous key range instead of folding the
    /// complete cross-document set and allocating [`Self::live_docs`].
    pub fn document_live(&self, document_hash: &str) -> Option<bool> {
        let mut tracked = false;
        for ((_, pid), set) in self
            .open_set
            .range((document_hash.to_owned(), Pid::MIN)..=(document_hash.to_owned(), Pid::MAX))
        {
            tracked = true;
            if set.present() && self.pid_alive(*pid) {
                return Some(true);
            }
        }
        tracked.then_some(false)
    }

    /// Every document with at least one present `(doc, pid)` (open-set ground truth,
    /// independent of alive) — the replacement for the `#lbreap` live-buffer scan.
    pub fn open_docs(&self) -> BTreeSet<String> {
        self.open_set
            .iter()
            .filter(|(_, set)| set.present())
            .map(|((doc, _), _)| doc.clone())
            .collect()
    }

    /// Derived live-doc aggregate: docs with a present `(doc, pid)` whose `pid` is
    /// alive. A dead pid's docs drop unless another live pid also holds them
    /// (whole-editor-death cascade, per-doc for free).
    pub fn live_docs(&self) -> BTreeSet<String> {
        self.open_set
            .iter()
            .filter(|((_, pid), set)| set.present() && self.pid_alive(*pid))
            .map(|((doc, _), _)| doc.clone())
            .collect()
    }

    /// Live/open editor registrations for `document_hash`, ordered
    /// deterministically by pid and registration value.
    pub fn live_registrations(&self, document_hash: &str) -> Vec<EditorRegistration> {
        self.registrations
            .iter()
            .filter(|((doc, pid), _)| {
                doc == document_hash && self.is_open(doc, *pid) && self.pid_alive(*pid)
            })
            .map(|(_, registration)| registration.clone())
            .collect()
    }

    /// `#ctrlkillreregister` Tier 3 — registrations that are live in the **replicated**
    /// plane but have no replica in the asking peer's `held` set.
    ///
    /// This is the replicated-Computed form of "who needs a replica rebuilt". The
    /// desired set is not controller-local state that has to be *sent* anywhere: it
    /// is the registration set, which already converges across processes as an OR-set
    /// plus LWW on this plane. So every peer derives the same answer from converged
    /// state, and the answer needs no request, no fan-out, and no delivery guarantee.
    ///
    /// That is what makes it strictly better than an effect that pushes a rebuild
    /// request. A push has to reach the other side — the failure mode behind
    /// `reload-lib reached 1/4 endpoints` — and only covers the moment it fires. A
    /// derivation over replicated state is correct whichever side restarted, and it
    /// self-heals for a registration that arrives later, because convergence does not
    /// care who was down.
    ///
    /// The controller calls this to learn which editors are stranded; the editor side
    /// calls it with its own pid and its own live replicas to discover that *it* is
    /// the one missing a replica, and repairs itself without being told.
    ///
    /// `held` is keyed by `document_hash`, matching [`EditorRegistration::document_hash`].
    pub fn registrations_missing_replica(
        &self,
        held: &BTreeSet<String>,
    ) -> Vec<EditorRegistration> {
        self.all_live_registrations()
            .into_iter()
            .filter(|registration| !held.contains(&registration.document_hash))
            .collect()
    }

    /// [`Self::registrations_missing_replica`] narrowed to one peer — the form an
    /// editor uses on itself.
    pub fn peer_registrations_missing_replica(
        &self,
        pid: Pid,
        held: &BTreeSet<String>,
    ) -> Vec<EditorRegistration> {
        self.registrations_missing_replica(held)
            .into_iter()
            .filter(|registration| registration.pid == pid)
            .collect()
    }

    /// Documents `pid` holds open with a registration, **regardless of whether
    /// `pid` is still alive**. This is the scope of a whole-editor death: the
    /// documents whose state must be re-derived once `pid` is dead. Unlike
    /// [`Self::all_live_registrations`] it does not depend on the death not having
    /// been folded yet, so a death observed through a second path first (a durable
    /// journal refresh replaying the same `Alive{false}`) still names every
    /// document the dead editor held.
    pub fn documents_registered_by_pid(&self, pid: Pid) -> BTreeSet<String> {
        self.registrations
            .keys()
            .filter(|(doc, registered_pid)| *registered_pid == pid && self.is_open(doc, pid))
            .map(|(doc, _)| doc.clone())
            .collect()
    }

    /// [`Self::documents_registered_by_pid`] with each document's registered
    /// path: `document_hash` → the path `pid`'s newest registration names.
    ///
    /// The path is the only liveness fact that ties a document to a project
    /// root. A controller uses it to keep its exit reaping and its durable
    /// exit writes inside its own root (`#exitwatcherrootscope`): the plane is
    /// process-global, so it can hold documents another root's controller
    /// registered.
    pub fn registered_document_paths_by_pid(&self, pid: Pid) -> BTreeMap<String, String> {
        self.registrations
            .iter()
            .filter(|((doc, registered_pid), _)| *registered_pid == pid && self.is_open(doc, pid))
            .map(|((doc, _), registration)| (doc.clone(), registration.path.clone()))
            .collect()
    }

    /// Live/open registrations across the whole projection.
    pub fn all_live_registrations(&self) -> Vec<EditorRegistration> {
        self.open_docs()
            .into_iter()
            .flat_map(|document_hash| self.live_registrations(&document_hash))
            .collect()
    }

    /// The plane's sync-in-flight signal: is any **live, open** editor holding
    /// unsynced edits (`edit_epoch > synced_epoch`) for `document_hash`? This is the
    /// derived-authority replacement for `editor_sync_in_flight` / the barrier's
    /// live-buffer scan (#sidecar-retirement): a disk write must not clobber an editor
    /// buffer that is ahead of the last synced epoch. A dead pid's unsynced edits do
    /// not count (its buffer is gone), matching the whole-editor-death cascade.
    pub fn document_in_flight(&self, document_hash: &str) -> bool {
        self.sync_state
            .range((document_hash.to_owned(), Pid::MIN)..=(document_hash.to_owned(), Pid::MAX))
            .any(|((doc, pid), reg)| {
                let (edit_epoch, synced_epoch) = *reg.value();
                edit_epoch > synced_epoch && self.is_open(doc, *pid) && self.pid_alive(*pid)
            })
    }

    /// The last reported `(edit_epoch, synced_epoch)` for `(document_hash, pid)`, or
    /// `None` if the editor never reported sync progress for the document.
    pub fn sync_epochs(&self, document_hash: &str, pid: Pid) -> Option<(u64, u64)> {
        self.sync_state
            .get(&(document_hash.to_string(), pid))
            .map(|reg| *reg.value())
    }
}

/// Sentinel `type_tag`-in-`node` marker distinguishing an agent-doc liveness
/// CrdtSync frame from a graph-state CrdtSync frame on a shared channel. agent-doc
/// runs liveness on its own reliable-sync channel, but tagging makes the decode
/// fail-closed rather than mis-fold a foreign frame.
const LIVENESS_NODE: NodeId = NodeId(u64::MAX);

/// Pack a `LivenessOp` batch into one `IpcMessage::CrdtSync` (spec § `#lzsync-liveness`:
/// liveness rides the CrdtSync plane). The batch is JSON bytes carried inline in a
/// single op's converged state, so it flows through the reliable-sync `SyncDriver` /
/// `DurableOutbox` exactly like any other frame; the driver hands the applied
/// `CrdtSync` back to the host, which folds it via [`decode_liveness_frame`].
pub fn encode_liveness_frame(ops: &[LivenessOp]) -> Result<IpcMessage> {
    let bytes = serde_json::to_vec(ops)?;
    let op = CrdtOp {
        node: LIVENESS_NODE,
        key: None,
        // The carriage stamp is unused by the fold (each `Alive` op carries its own
        // LWW stamp); zero is a stable, deterministic placeholder.
        stamp: WireStamp {
            wall_time: 0,
            logical: 0,
            peer: 0,
        },
        state: IpcValue::from(bytes),
    };
    Ok(IpcMessage::CrdtSync(CrdtSync {
        frontier: Vec::new(),
        ops: vec![op],
    }))
}

/// Decode a liveness frame produced by [`encode_liveness_frame`].
///
/// Returns `None` for a non-`CrdtSync` message or a `CrdtSync` that carries no
/// liveness-tagged op (a foreign/graph frame — let the host route it elsewhere);
/// `Some(Err)` for a liveness-tagged op whose inline bytes are malformed (never a
/// silent drop). The `SharedBlob` transport is not used for liveness (small frames),
/// so a `SharedBlob` op on the liveness node is treated as malformed.
pub fn decode_liveness_frame(message: &IpcMessage) -> Option<Result<Vec<LivenessOp>>> {
    let IpcMessage::CrdtSync(sync) = message else {
        return None;
    };
    let mut liveness_ops: Vec<&CrdtOp> = sync
        .ops
        .iter()
        .filter(|op| op.node == LIVENESS_NODE)
        .collect();
    if liveness_ops.is_empty() {
        return None;
    }
    // Keep op order (later frames override earlier via LWW/OR-set anyway).
    liveness_ops.sort_by_key(|op| op.stamp);
    Some(decode_liveness_ops(&liveness_ops))
}

fn decode_liveness_ops(ops: &[&CrdtOp]) -> Result<Vec<LivenessOp>> {
    let mut out = Vec::new();
    for op in ops {
        let IpcValue::Inline(bytes) = &op.state else {
            return Err(anyhow!(
                "liveness frame carried a non-inline (SharedBlob) op state"
            ));
        };
        let batch: Vec<LivenessOp> = serde_json::from_slice(bytes)
            .map_err(|e| anyhow!("liveness frame inline decode failed: {e}"))?;
        out.extend(batch);
    }
    Ok(out)
}

/// The LWW cell an op writes, or `None` for an OR-set op (`Open` / `Close`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum LwwCell<'a> {
    Alive(Pid),
    Sync(&'a str, Pid),
    Register(&'a str, Pid),
}

/// The candidate that currently wins one LWW cell during a journal replay.
enum LwwWinner<'a> {
    Stamped(WireStamp),
    Registration(&'a EditorRegistration),
}

impl LwwWinner<'_> {
    /// Exactly [`LivenessProjection::apply`]'s replacement rule: a stamped
    /// register adopts only a strictly higher stamp, and a registration adopts a
    /// strictly greater `(timestamp_ms, value)`. Ties keep the earlier write.
    fn replaced_by(&self, other: &LwwWinner<'_>) -> bool {
        match (self, other) {
            (Self::Stamped(current), LwwWinner::Stamped(next)) => next > current,
            (Self::Registration(current), LwwWinner::Registration(next)) => {
                next.timestamp_ms > current.timestamp_ms
                    || (next.timestamp_ms == current.timestamp_ms && *next > *current)
            }
            // Cells are keyed by op kind, so the two shapes never share a cell.
            _ => false,
        }
    }
}

fn lww_cell(op: &LivenessOp) -> Option<(LwwCell<'_>, LwwWinner<'_>)> {
    match op {
        LivenessOp::Open { .. } | LivenessOp::Close { .. } => None,
        LivenessOp::Alive { pid, stamp, .. } => {
            Some((LwwCell::Alive(*pid), LwwWinner::Stamped(*stamp)))
        }
        LivenessOp::Sync {
            document_hash,
            pid,
            stamp,
            ..
        } => Some((
            LwwCell::Sync(document_hash, *pid),
            LwwWinner::Stamped(*stamp),
        )),
        LivenessOp::Register(registration) => Some((
            LwwCell::Register(&registration.document_hash, registration.pid),
            LwwWinner::Registration(registration),
        )),
    }
}

/// Indexes of journal batches whose every op is already dominated by a write in
/// another retained batch (`#sdkingressstall`).
///
/// The durable liveness journal is replayed batch-by-batch, in order, into a
/// [`LivenessProjection`]. Editor heartbeats re-send `Register` (and `Alive` /
/// `Sync`) for the same cell every few seconds, so without compaction the
/// journal grows without bound — 497,353 rows / 305 MB on `haiven-dev`, of which
/// 1,037 cells carried the whole projection — and every cold replay decodes all
/// of it.
///
/// A batch is removable only when **all** of these hold, which makes replaying
/// the retained batches (in their original order) fold to exactly the same
/// projection as replaying every batch:
///
/// - it is not `pinned` (the caller pins each source's newest frame so a
///   redelivered frame still meets its stored payload);
/// - it carries no OR-set op (`Open`/`Close` tags accumulate; every tag counts);
/// - none of its LWW ops is the *winning* write of its cell, where the winner is
///   the first write, in replay order, that no later write replaces under
///   [`LivenessProjection::apply`]'s own rule.
pub fn superseded_liveness_batches(
    batches: &[Vec<LivenessOp>],
    pinned: &BTreeSet<usize>,
) -> BTreeSet<usize> {
    let mut winners: BTreeMap<LwwCell<'_>, (LwwWinner<'_>, usize)> = BTreeMap::new();
    for (index, batch) in batches.iter().enumerate() {
        for op in batch {
            let Some((cell, candidate)) = lww_cell(op) else {
                continue;
            };
            match winners.get_mut(&cell) {
                Some((current, holder)) => {
                    if current.replaced_by(&candidate) {
                        *current = candidate;
                        *holder = index;
                    }
                }
                None => {
                    winners.insert(cell, (candidate, index));
                }
            }
        }
    }
    let winning_batches = winners
        .values()
        .map(|(_, holder)| *holder)
        .collect::<BTreeSet<_>>();
    batches
        .iter()
        .enumerate()
        .filter(|(index, batch)| {
            !pinned.contains(index)
                && !winning_batches.contains(index)
                && batch.iter().all(|op| lww_cell(op).is_some())
        })
        .map(|(index, _)| index)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp(wall: u64, peer: u64) -> WireStamp {
        WireStamp {
            wall_time: wall,
            logical: 0,
            peer,
        }
    }

    /// GH #236: the native-save fence admits by protocol, not by equality with
    /// the classic package version.
    #[test]
    fn gh236_native_save_compatibility_follows_the_protocol_not_the_classic_version() {
        let classic = expected_editor_plugin_version("jetbrains")
            .expect("classic JetBrains package version baked by build.rs");
        let paired = release_paired_editor_plugin_versions("jetbrains");
        assert_eq!(paired.len(), 3, "all JetBrains package lines are baked in");
        assert_eq!(paired.iter().copied().collect::<BTreeSet<_>>().len(), 3);
        let modular = paired[1];
        let modern = paired[2];

        assert_eq!(
            expected_editor_plugin_version_for_platform_build("jetbrains", Some(261)),
            Some(classic)
        );
        assert_eq!(
            expected_editor_plugin_version_for_platform_build("jetbrains", Some(262)),
            Some(modular)
        );
        assert_eq!(
            expected_editor_plugin_version_for_platform_build("jetbrains", Some(263)),
            Some(modern)
        );

        // Matching classic package.
        assert_eq!(
            editor_native_save_compatibility("jetbrains", classic),
            NativeSaveCompatibility::ReleasePaired
        );
        // The release-paired 262 package can never equal the classic version.
        assert_eq!(
            editor_native_save_compatibility("jetbrains", modular),
            NativeSaveCompatibility::ReleasePaired
        );
        assert!(editor_plugin_version_is_release_paired(
            "JetBrains",
            modular
        ));
        assert!(!editor_plugin_generation_matches("jetbrains", modular));
        assert_eq!(
            editor_native_save_compatibility("jetbrains", modern),
            NativeSaveCompatibility::ReleasePaired
        );
        // Older 263 installations remain protocol-compatible during rollout.
        let gh236 = editor_native_save_compatibility("jetbrains", "0.2.508");
        assert_eq!(
            gh236,
            NativeSaveCompatibility::ProtocolCompatible { floor: "0.2.497" }
        );
        assert!(gh236.admits_native_save());
        // The floor itself is compatible; the build just before it is not.
        assert!(editor_native_save_compatibility("jetbrains", "0.2.497").admits_native_save());
        // A truly incompatible plugin: it answers a slow save `rejected`.
        let old = editor_native_save_compatibility("jetbrains", "0.2.496");
        assert_eq!(
            old,
            NativeSaveCompatibility::BelowProtocolFloor { floor: "0.2.497" }
        );
        assert!(!old.admits_native_save());
        assert_eq!(old.token(), "below_native_save_protocol_floor");
        // Unparsable versions and unknown kinds fail closed.
        assert!(!editor_native_save_compatibility("jetbrains", "unknown").admits_native_save());
        assert!(!editor_native_save_compatibility("jetbrains", "").admits_native_save());
        assert_eq!(
            editor_native_save_compatibility("zed", "9.9.9"),
            NativeSaveCompatibility::UnknownKind
        );
        // VS Code: 0.2.66 shipped both before and after persist_current existed.
        assert!(!editor_native_save_compatibility("vscode", "0.2.66").admits_native_save());
        assert!(editor_native_save_compatibility("vscode", "0.2.67").admits_native_save());
        // Suffixes and a `v` prefix do not change the verdict.
        assert!(editor_native_save_compatibility("jb", "v0.2.508+local").admits_native_save());
        assert!(editor_native_save_compatibility("jetbrains", "0.3").admits_native_save());
    }

    #[test]
    fn editor_registration_is_visible_only_while_replica_is_live_and_open() {
        let mut projection = LivenessProjection::new();
        let registration = EditorRegistration {
            document_hash: "docA".into(),
            pid: 100,
            path: "/tmp/doc.md".into(),
            editor_id: "jetbrains-100-test".into(),
            editor_kind: "jetbrains".into(),
            editor_version: "0.2.270".into(),
            capabilities: vec!["operator_text_authority_v1".into()],
            timestamp_ms: 10,
        };
        projection.apply(&LivenessOp::Register(registration.clone()));
        assert!(projection.live_registrations("docA").is_empty());

        projection.apply(&LivenessOp::Open {
            document_hash: "docA".into(),
            pid: 100,
            tag: "open-1".into(),
        });
        assert_eq!(projection.live_registrations("docA"), vec![registration]);

        projection.apply(&LivenessOp::Alive {
            pid: 100,
            value: false,
            stamp: stamp(20, 1),
        });
        assert!(projection.live_registrations("docA").is_empty());
    }

    #[test]
    fn editor_registration_advances_by_timestamp_before_metadata_order() {
        let mut projection = LivenessProjection::new();
        projection.apply(&LivenessOp::Open {
            document_hash: "docA".into(),
            pid: 100,
            tag: "open-1".into(),
        });
        let older = EditorRegistration {
            document_hash: "docA".into(),
            pid: 100,
            path: "/tmp/z-old.md".into(),
            editor_id: "z-old".into(),
            editor_kind: "jetbrains".into(),
            editor_version: "9.9.9".into(),
            capabilities: vec!["z-old".into()],
            timestamp_ms: 10,
        };
        let newer = EditorRegistration {
            path: "/tmp/a-new.md".into(),
            editor_id: "a-new".into(),
            editor_version: "0.2.270".into(),
            capabilities: vec!["a-new".into()],
            timestamp_ms: 20,
            ..older.clone()
        };

        projection.apply(&LivenessOp::Register(older));
        projection.apply(&LivenessOp::Register(newer.clone()));
        assert_eq!(projection.live_registrations("docA"), vec![newer.clone()]);

        projection.apply(&LivenessOp::Register(EditorRegistration {
            timestamp_ms: 5,
            path: "/tmp/zz-stale.md".into(),
            ..newer.clone()
        }));
        assert_eq!(projection.live_registrations("docA"), vec![newer]);
    }

    /// `#ctrlkillreregister` Tier 3 — the missing-replica set is DERIVED from
    /// replicated state, so both sides compute the same answer without either one
    /// sending a request.
    ///
    /// This is what makes it better than pushing a rebuild: a push must reach the
    /// other side and only covers the instant it fires, whereas this is correct
    /// whichever peer restarted and self-heals for registrations that arrive later.
    #[test]
    fn missing_replica_set_is_derived_from_replicated_registrations() {
        let mut projection = LivenessProjection::new();
        for (doc, pid, editor_id) in [
            ("docA", 1u64, "jetbrains-a"),
            ("docB", 1u64, "jetbrains-a"),
            ("docC", 7u64, "vscode-c"),
        ] {
            projection.apply(&LivenessOp::Open {
                document_hash: doc.into(),
                pid,
                tag: format!("open-{doc}-{pid}"),
            });
            projection.apply(&LivenessOp::Register(EditorRegistration {
                document_hash: doc.into(),
                pid,
                path: format!("/proj/{doc}.md"),
                editor_id: editor_id.into(),
                editor_kind: "jetbrains".into(),
                editor_version: "test".into(),
                capabilities: vec![],
                timestamp_ms: 1,
            }));
        }

        // A peer holding every replica it registered needs nothing.
        let all: BTreeSet<String> = ["docA", "docB", "docC"]
            .into_iter()
            .map(str::to_string)
            .collect();
        assert!(projection.registrations_missing_replica(&all).is_empty());

        // A controller that lost its hub (the process-local part that does NOT
        // survive a restart) derives exactly who is stranded, with no message sent.
        let none: BTreeSet<String> = BTreeSet::new();
        let mut stranded: Vec<String> = projection
            .registrations_missing_replica(&none)
            .into_iter()
            .map(|registration| registration.document_hash)
            .collect();
        stranded.sort();
        assert_eq!(stranded, vec!["docA", "docB", "docC"]);

        // An editor asks the same question about ITSELF and repairs without being
        // told — the peer-scoped form.
        let held_b_only: BTreeSet<String> = ["docB"].into_iter().map(str::to_string).collect();
        let mine: Vec<String> = projection
            .peer_registrations_missing_replica(1, &held_b_only)
            .into_iter()
            .map(|registration| registration.document_hash)
            .collect();
        assert_eq!(
            mine,
            vec!["docA".to_string()],
            "only this peer's own un-held registration, not another peer's"
        );

        // A dead peer is not stranded — it has nothing to rebuild.
        projection.apply(&LivenessOp::Alive {
            pid: 7,
            value: false,
            stamp: stamp(30, 1),
        });
        let after_death: Vec<String> = projection
            .registrations_missing_replica(&none)
            .into_iter()
            .map(|registration| registration.document_hash)
            .collect();
        assert_eq!(after_death, vec!["docA".to_string(), "docB".to_string()]);
    }

    // Conformance scenario `open_set_add_wins_over_stale_remove`
    // (lazily-spec/conformance/reliable-sync/liveness_orset_lww.json).
    #[test]
    fn open_set_add_wins_over_stale_remove() {
        let mut p = LivenessProjection::new();
        p.apply(&LivenessOp::Open {
            document_hash: "docA".into(),
            pid: 100,
            tag: "t1".into(),
        });
        p.apply(&LivenessOp::Close {
            document_hash: "docA".into(),
            pid: 100,
            observed_tags: vec!["t1".into()],
        });
        p.apply(&LivenessOp::Open {
            document_hash: "docA".into(),
            pid: 100,
            tag: "t3".into(),
        });
        assert!(
            p.is_open("docA", 100),
            "re-open tag t3 not observed by the close"
        );
    }

    fn open(p: &mut LivenessProjection, doc: &str, pid: Pid) {
        p.apply(&LivenessOp::Open {
            document_hash: doc.into(),
            pid,
            tag: format!("open-{pid}"),
        });
    }

    // #sidecar-retirement sync-in-flight foundation.
    #[test]
    fn sync_in_flight_true_when_edit_ahead_of_synced() {
        let mut p = LivenessProjection::new();
        open(&mut p, "docA", 100);
        p.apply(&LivenessOp::Sync {
            document_hash: "docA".into(),
            pid: 100,
            edit_epoch: 3,
            synced_epoch: 1,
            stamp: stamp(10, 1),
        });
        assert!(p.document_in_flight("docA"));
        assert_eq!(p.sync_epochs("docA", 100), Some((3, 1)));
    }

    #[test]
    fn sync_not_in_flight_when_synced_catches_up_lww() {
        let mut p = LivenessProjection::new();
        open(&mut p, "docA", 100);
        p.apply(&LivenessOp::Sync {
            document_hash: "docA".into(),
            pid: 100,
            edit_epoch: 3,
            synced_epoch: 1,
            stamp: stamp(10, 1),
        });
        // A later report at a higher stamp: the editor confirmed sync through 3.
        p.apply(&LivenessOp::Sync {
            document_hash: "docA".into(),
            pid: 100,
            edit_epoch: 3,
            synced_epoch: 3,
            stamp: stamp(20, 1),
        });
        assert!(!p.document_in_flight("docA"));
    }

    #[test]
    fn sync_lww_highest_stamp_wins_regardless_of_order_and_redelivery_is_noop() {
        let mut p = LivenessProjection::new();
        open(&mut p, "docA", 100);
        let newer = LivenessOp::Sync {
            document_hash: "docA".into(),
            pid: 100,
            edit_epoch: 5,
            synced_epoch: 5,
            stamp: stamp(30, 1),
        };
        let older = LivenessOp::Sync {
            document_hash: "docA".into(),
            pid: 100,
            edit_epoch: 4,
            synced_epoch: 2,
            stamp: stamp(20, 1),
        };
        // Apply newer first, then the older (out of order) — the higher stamp holds.
        p.apply(&newer);
        p.apply(&older);
        assert_eq!(p.sync_epochs("docA", 100), Some((5, 5)));
        assert!(!p.document_in_flight("docA"));
        // Redelivery of the older op is a no-op.
        p.apply(&older);
        assert_eq!(p.sync_epochs("docA", 100), Some((5, 5)));
    }

    #[test]
    fn sync_in_flight_ignores_dead_or_closed_editor() {
        // A closed editor's unsynced edits do not count.
        let mut closed = LivenessProjection::new();
        closed.apply(&LivenessOp::Sync {
            document_hash: "docA".into(),
            pid: 100,
            edit_epoch: 3,
            synced_epoch: 1,
            stamp: stamp(10, 1),
        });
        assert!(
            !closed.document_in_flight("docA"),
            "no open fact for the pid ⇒ not in flight"
        );
        // A dead editor's unsynced edits do not count (whole-editor-death cascade).
        let mut dead = LivenessProjection::new();
        open(&mut dead, "docA", 100);
        dead.apply(&LivenessOp::Sync {
            document_hash: "docA".into(),
            pid: 100,
            edit_epoch: 3,
            synced_epoch: 1,
            stamp: stamp(10, 1),
        });
        assert!(dead.document_in_flight("docA"));
        dead.apply(&LivenessOp::Alive {
            pid: 100,
            value: false,
            stamp: stamp(15, 1),
        });
        assert!(
            !dead.document_in_flight("docA"),
            "a dead pid's in-flight edits drop"
        );
    }

    #[test]
    fn sync_op_round_trips_through_the_liveness_frame() {
        let ops = vec![LivenessOp::Sync {
            document_hash: "docA".into(),
            pid: 100,
            edit_epoch: 7,
            synced_epoch: 4,
            stamp: stamp(10, 2),
        }];
        let frame = encode_liveness_frame(&ops).expect("encode");
        let decoded = decode_liveness_frame(&frame)
            .expect("liveness frame")
            .expect("decode");
        assert_eq!(decoded, ops);
    }

    #[test]
    fn closed_document_remains_tracked_for_authoritative_false_reads() {
        let mut projection = LivenessProjection::new();
        projection.apply(&LivenessOp::Close {
            document_hash: "known-closed".into(),
            pid: 9,
            observed_tags: vec!["old-open".into()],
        });

        assert!(projection.tracks_document("known-closed"));
        assert!(!projection.open_docs().contains("known-closed"));
        assert!(!projection.tracks_document("never-seen"));
    }

    #[test]
    fn document_live_ranges_one_document_and_preserves_cold_vs_closed() {
        let mut projection = LivenessProjection::new();
        for index in 0..1_000 {
            open(&mut projection, &format!("other-{index:04}"), index + 100);
        }
        open(&mut projection, "target", 7);
        open(&mut projection, "target-prefix", 8);

        assert_eq!(projection.document_live("target"), Some(true));
        assert_eq!(projection.document_live("target-prefix"), Some(true));
        assert_eq!(projection.document_live("never-seen"), None);

        projection.apply(&LivenessOp::Close {
            document_hash: "target".into(),
            pid: 7,
            observed_tags: vec!["open-7".into()],
        });
        assert_eq!(
            projection.document_live("target"),
            Some(false),
            "a tracked, known-closed document is authoritative false, not cold"
        );
        assert_eq!(
            projection.document_live("target-prefix"),
            Some(true),
            "an adjacent key prefix must not bleed into the target range"
        );

        open(&mut projection, "target", 9);
        projection.apply(&LivenessOp::Alive {
            pid: 9,
            value: false,
            stamp: stamp(20, 1),
        });
        assert_eq!(
            projection.document_live("target"),
            Some(false),
            "a dead editor keeps the document tracked without making it live"
        );
    }

    #[test]
    fn open_pid_index_tracks_only_real_membership_edges() {
        let mut projection = LivenessProjection::new();
        let open_a = LivenessOp::Open {
            document_hash: "doc-a".into(),
            pid: 42,
            tag: "a1".into(),
        };
        let open_b = LivenessOp::Open {
            document_hash: "doc-b".into(),
            pid: 42,
            tag: "b1".into(),
        };
        projection.apply(&open_a);
        projection.apply(&open_b);
        projection.apply(&open_a);
        assert_eq!(projection.open_pid_counts.get(&42), Some(&2));
        assert_eq!(projection.all_open_pids(), [42].into_iter().collect());

        projection.apply(&LivenessOp::Close {
            document_hash: "doc-a".into(),
            pid: 42,
            observed_tags: vec!["older-unseen-tag".into()],
        });
        assert_eq!(
            projection.open_pid_counts.get(&42),
            Some(&2),
            "a stale close that changes no membership must not decrement"
        );

        let close_a = LivenessOp::Close {
            document_hash: "doc-a".into(),
            pid: 42,
            observed_tags: vec!["a1".into()],
        };
        let close_b = LivenessOp::Close {
            document_hash: "doc-b".into(),
            pid: 42,
            observed_tags: vec!["b1".into()],
        };
        projection.apply(&close_a);
        assert_eq!(projection.open_pid_counts.get(&42), Some(&1));
        projection.apply(&close_b);
        projection.apply(&close_b);
        assert!(!projection.open_pid_counts.contains_key(&42));
        assert!(projection.all_open_pids().is_empty());

        projection.apply(&LivenessOp::Open {
            document_hash: "doc-a".into(),
            pid: 42,
            tag: "a2".into(),
        });
        projection.apply(&close_a);
        assert_eq!(
            projection.all_open_pids(),
            [42].into_iter().collect(),
            "a fresh reopen is add-wins over a repeated close of the old tag"
        );
    }

    #[test]
    fn apply_batch_hydration_builds_the_open_pid_index() {
        let mut projection = LivenessProjection::new();
        projection.apply_batch(&[
            LivenessOp::Open {
                document_hash: "doc-a".into(),
                pid: 42,
                tag: "a1".into(),
            },
            LivenessOp::Open {
                document_hash: "doc-b".into(),
                pid: 42,
                tag: "b1".into(),
            },
            LivenessOp::Open {
                document_hash: "doc-c".into(),
                pid: 77,
                tag: "c1".into(),
            },
            LivenessOp::Close {
                document_hash: "doc-b".into(),
                pid: 42,
                observed_tags: vec!["b1".into()],
            },
        ]);

        assert_eq!(projection.open_pid_counts.get(&42), Some(&1));
        assert_eq!(projection.open_pid_counts.get(&77), Some(&1));
        assert_eq!(projection.all_open_pids(), [42, 77].into_iter().collect());
    }

    #[test]
    fn document_in_flight_range_does_not_adopt_an_adjacent_document() {
        let mut projection = LivenessProjection::new();
        open(&mut projection, "target", 7);
        open(&mut projection, "target-prefix", 8);
        projection.apply(&LivenessOp::Sync {
            document_hash: "target".into(),
            pid: 7,
            edit_epoch: 3,
            synced_epoch: 3,
            stamp: stamp(10, 1),
        });
        projection.apply(&LivenessOp::Sync {
            document_hash: "target-prefix".into(),
            pid: 8,
            edit_epoch: 4,
            synced_epoch: 1,
            stamp: stamp(10, 1),
        });

        assert!(!projection.document_in_flight("target"));
        assert!(projection.document_in_flight("target-prefix"));
    }

    // Order independence + re-delivery no-op for the same scenario.
    #[test]
    fn open_set_order_independent_and_redelivery_noop() {
        let ops = [
            LivenessOp::Open {
                document_hash: "docA".into(),
                pid: 100,
                tag: "t3".into(),
            },
            LivenessOp::Close {
                document_hash: "docA".into(),
                pid: 100,
                observed_tags: vec!["t1".into()],
            },
            LivenessOp::Open {
                document_hash: "docA".into(),
                pid: 100,
                tag: "t1".into(),
            },
        ];
        let mut p = LivenessProjection::new();
        for op in &ops {
            p.apply(op);
        }
        // Re-deliver everything again, reversed — must not change the result.
        for op in ops.iter().rev() {
            p.apply(op);
        }
        assert!(p.is_open("docA", 100));
    }

    /// The documents a dead editor held stay nameable after its death is folded,
    /// so an exit observed through a second path first still re-derives them.
    #[test]
    fn documents_registered_by_pid_survive_the_pid_death() {
        let mut p = LivenessProjection::new();
        for doc in ["docA", "docB"] {
            p.apply(&LivenessOp::Open {
                document_hash: doc.into(),
                pid: 100,
                tag: format!("{doc}-open"),
            });
            p.apply(&LivenessOp::Register(registration(doc, 100, "jb-100", 1)));
        }
        // Registered but closed: not held.
        p.apply(&LivenessOp::Register(registration(
            "docC", 100, "jb-100", 1,
        )));
        // Another editor's document.
        p.apply(&LivenessOp::Open {
            document_hash: "docD".into(),
            pid: 200,
            tag: "docD-open".into(),
        });
        p.apply(&LivenessOp::Register(registration(
            "docD", 200, "jb-200", 1,
        )));
        let held = BTreeSet::from(["docA".to_string(), "docB".to_string()]);
        assert_eq!(p.documents_registered_by_pid(100), held);

        p.apply(&LivenessOp::Alive {
            pid: 100,
            value: false,
            stamp: stamp(10, 1),
        });
        assert!(p.all_live_registrations().iter().all(|r| r.pid != 100));
        assert_eq!(p.documents_registered_by_pid(100), held);
        // `#exitwatcherrootscope`: the same held set, each with the path that
        // ties it to a project root.
        assert_eq!(
            p.registered_document_paths_by_pid(100),
            BTreeMap::from([
                ("docA".to_string(), "/tmp/docA.md".to_string()),
                ("docB".to_string(), "/tmp/docB.md".to_string()),
            ])
        );
    }

    // Conformance scenario `lww_alive_highest_stamp_wins`.
    #[test]
    fn lww_alive_highest_stamp_wins() {
        let mut p = LivenessProjection::new();
        p.apply(&LivenessOp::Alive {
            pid: 100,
            value: true,
            stamp: stamp(20, 1),
        });
        p.apply(&LivenessOp::Alive {
            pid: 100,
            value: false,
            stamp: stamp(25, 1),
        });
        // Stale re-assert at a lower stamp is dominated.
        p.apply(&LivenessOp::Alive {
            pid: 100,
            value: true,
            stamp: stamp(22, 1),
        });
        assert!(!p.pid_alive(100), "highest-stamp alive=false wins");
    }

    // Conformance scenario `whole_editor_death_cascades`.
    #[test]
    fn whole_editor_death_cascades() {
        let mut p = LivenessProjection::new();
        for (doc, pid, tag) in [("docA", 100, "a"), ("docB", 100, "b"), ("docC", 200, "c")] {
            p.apply(&LivenessOp::Open {
                document_hash: doc.into(),
                pid,
                tag: tag.into(),
            });
        }
        assert_eq!(
            p.live_docs(),
            ["docA", "docB", "docC"]
                .map(String::from)
                .into_iter()
                .collect()
        );
        // pid 100 dies → docA and docB drop; docC (pid 200) unaffected.
        p.apply(&LivenessOp::Alive {
            pid: 100,
            value: false,
            stamp: stamp(30, 1),
        });
        assert_eq!(
            p.live_docs(),
            ["docC"].map(String::from).into_iter().collect()
        );
        // open_docs (open-set ground truth) still shows all three — death is not a close.
        assert_eq!(p.open_docs().len(), 3);
        assert_eq!(
            p.all_open_pids(),
            [100, 200].into_iter().collect(),
            "restore-time OS reconciliation must still see a dead pid whose durable open facts remain"
        );
    }

    // A doc shared by a dead pid and a live pid stays live (cascade is per-pid).
    #[test]
    fn shared_doc_stays_live_via_second_pid() {
        let mut p = LivenessProjection::new();
        p.apply(&LivenessOp::Open {
            document_hash: "docA".into(),
            pid: 100,
            tag: "a".into(),
        });
        p.apply(&LivenessOp::Open {
            document_hash: "docA".into(),
            pid: 200,
            tag: "b".into(),
        });
        p.apply(&LivenessOp::Alive {
            pid: 100,
            value: false,
            stamp: stamp(30, 1),
        });
        assert!(
            p.live_docs().contains("docA"),
            "pid 200 still alive keeps docA live"
        );
        assert_eq!(p.open_pids("docA"), [100, 200].into_iter().collect());
    }

    // `#ghosteditorliveness` regression: a document restored by durable hydration
    // (controller restart) carries `Open` + `Register` facts but NO `Alive` fact,
    // because the crashed editor's exit watcher never published its terminal
    // `Alive{false}`. `pid_alive` presumes alive absent a death fact, so the pid is
    // a *ghost* — counted live forever, holding `live_editors >= 1` and wedging
    // every disk-authority resolve. The exit-watcher reconciliation must publish the
    // missing death fact (from OS `kill(pid,0)` == ESRCH), after which the ghost
    // drops from every live view. `all_open_pids` must still surface the pid BEFORE
    // reconciliation so the watcher can find it to reap.
    #[test]
    fn hydrated_open_without_alive_is_a_ghost_until_reaped() {
        let mut p = LivenessProjection::new();
        // Hydration replays the durable Open + Register with no Alive fact.
        p.apply(&LivenessOp::Open {
            document_hash: "docA".into(),
            pid: 930287,
            tag: "boot".into(),
        });
        p.apply(&LivenessOp::Register(EditorRegistration {
            document_hash: "docA".into(),
            pid: 930287,
            path: "/proj/tasks/plan.md".into(),
            editor_id: "jetbrains".into(),
            editor_kind: "jetbrains".into(),
            editor_version: "2024.2".into(),
            capabilities: vec![],
            timestamp_ms: 1,
        }));

        // Ghost: with no death fact the crashed editor reads as fully live.
        assert!(
            p.pid_alive(930287),
            "no death fact ⇒ presumed alive (the bug)"
        );
        assert!(
            p.live_docs().contains("docA"),
            "ghost holds live_editors>=1"
        );
        assert_eq!(
            p.live_registrations("docA").len(),
            1,
            "ghost registration is counted as a delivery target"
        );
        // The watcher's reap candidate source must still see the ghost pid.
        assert!(
            p.all_open_pids().contains(&930287),
            "all_open_pids must surface the ghost so the watcher can reap it"
        );

        // Reconciliation: OS liveness says the pid is gone → publish the death fact
        // the crashed editor never sent.
        p.apply(&LivenessOp::Alive {
            pid: 930287,
            value: false,
            stamp: stamp(100, 0),
        });

        assert!(
            !p.pid_alive(930287),
            "reap publishes the missing Alive{{false}}"
        );
        assert!(
            !p.live_docs().contains("docA"),
            "reaped ghost drops from live_docs ⇒ disk authority ⇒ convergence"
        );
        assert!(
            p.live_registrations("docA").is_empty(),
            "reaped ghost is no longer a delivery target"
        );
    }

    #[test]
    fn liveness_frame_roundtrips_through_crdtsync() {
        let batch = vec![
            LivenessOp::Open {
                document_hash: "docA".into(),
                pid: 100,
                tag: "t1".into(),
            },
            LivenessOp::Alive {
                pid: 100,
                value: false,
                stamp: stamp(25, 1),
            },
        ];
        let frame = encode_liveness_frame(&batch).expect("encode");
        assert!(matches!(frame, IpcMessage::CrdtSync(_)));
        let decoded = decode_liveness_frame(&frame)
            .expect("is liveness")
            .expect("ok");
        assert_eq!(decoded, batch);
    }

    #[test]
    fn decode_ignores_foreign_crdtsync_and_non_crdtsync() {
        // A non-CrdtSync message.
        let snap = IpcMessage::Snapshot(lazily::Snapshot::new(0, vec![], vec![], vec![]));
        assert!(decode_liveness_frame(&snap).is_none());
        // A CrdtSync with no liveness-tagged op (foreign graph frame).
        let foreign = IpcMessage::CrdtSync(CrdtSync {
            frontier: Vec::new(),
            ops: vec![CrdtOp {
                node: NodeId(7),
                key: None,
                stamp: stamp(1, 1),
                state: IpcValue::from(vec![1, 2, 3]),
            }],
        });
        assert!(decode_liveness_frame(&foreign).is_none());
    }

    #[test]
    fn decode_surfaces_malformed_liveness_bytes() {
        let bad = IpcMessage::CrdtSync(CrdtSync {
            frontier: Vec::new(),
            ops: vec![CrdtOp {
                node: LIVENESS_NODE,
                key: None,
                stamp: stamp(1, 1),
                state: IpcValue::from(b"not json".to_vec()),
            }],
        });
        assert!(decode_liveness_frame(&bad).expect("is liveness").is_err());
    }

    // End-to-end fold from a decoded frame (the controller's receive path).
    #[test]
    fn projection_folds_a_decoded_frame() {
        let batch = vec![
            LivenessOp::Open {
                document_hash: "docA".into(),
                pid: 100,
                tag: "t1".into(),
            },
            LivenessOp::Open {
                document_hash: "docB".into(),
                pid: 100,
                tag: "t2".into(),
            },
        ];
        let frame = encode_liveness_frame(&batch).unwrap();
        let ops = decode_liveness_frame(&frame).unwrap().unwrap();
        let mut p = LivenessProjection::new();
        p.apply_batch(&ops);
        assert_eq!(
            p.open_docs(),
            ["docA", "docB"].map(String::from).into_iter().collect()
        );
    }

    fn registration(document_hash: &str, pid: Pid, editor_id: &str, ts: u64) -> EditorRegistration {
        EditorRegistration {
            document_hash: document_hash.into(),
            pid,
            path: format!("/tmp/{document_hash}.md"),
            editor_id: editor_id.into(),
            editor_kind: "jetbrains".into(),
            editor_version: "0.2.464".into(),
            capabilities: Vec::new(),
            timestamp_ms: ts,
        }
    }

    fn fold(batches: &[Vec<LivenessOp>]) -> LivenessProjection {
        let mut projection = LivenessProjection::new();
        for batch in batches {
            projection.apply_batch(batch);
        }
        projection
    }

    /// `#sdkingressstall`: the heartbeat-shaped journal compacts to its winning
    /// writes, and replaying only the retained batches folds to the identical
    /// projection — including ties, where `apply` keeps the EARLIER write.
    #[test]
    fn superseded_liveness_batches_preserve_the_folded_projection() {
        let mut batches: Vec<Vec<LivenessOp>> = vec![vec![LivenessOp::Open {
            document_hash: "docA".into(),
            pid: 7,
            tag: "open-1".into(),
        }]];
        for ts in 1..=200 {
            batches.push(vec![LivenessOp::Register(registration(
                "docA", 7, "ed", ts,
            ))]);
            batches.push(vec![LivenessOp::Sync {
                document_hash: "docA".into(),
                pid: 7,
                edit_epoch: ts,
                synced_epoch: ts - 1,
                stamp: stamp(ts, 0),
            }]);
        }
        // Equal-stamp Alive writes with different values: the first one applied wins.
        batches.push(vec![LivenessOp::Alive {
            pid: 9,
            value: false,
            stamp: stamp(50, 1),
        }]);
        batches.push(vec![LivenessOp::Alive {
            pid: 9,
            value: true,
            stamp: stamp(50, 1),
        }]);
        // An older registration arriving late, and an equal-timestamp one that
        // wins only by value order.
        batches.push(vec![LivenessOp::Register(registration("docA", 7, "ed", 3))]);
        batches.push(vec![LivenessOp::Register(registration(
            "docA", 7, "zz", 200,
        ))]);
        batches.push(vec![LivenessOp::Close {
            document_hash: "docA".into(),
            pid: 7,
            observed_tags: vec!["open-1".into()],
        }]);
        // A mixed batch whose LWW op is stale is still kept for its OR-set op.
        batches.push(vec![
            LivenessOp::Register(registration("docA", 7, "ed", 4)),
            LivenessOp::Open {
                document_hash: "docA".into(),
                pid: 7,
                tag: "open-2".into(),
            },
        ]);
        let pinned = BTreeSet::from([5]);

        let removable = superseded_liveness_batches(&batches, &pinned);
        let retained = batches
            .iter()
            .enumerate()
            .filter(|(index, _)| !removable.contains(index))
            .map(|(_, batch)| batch.clone())
            .collect::<Vec<_>>();

        assert_eq!(fold(&retained), fold(&batches));
        assert!(!removable.contains(&5), "a pinned batch is never removed");
        assert!(!removable.contains(&0), "OR-set batches are never removed");
        assert!(
            retained.len() <= 8,
            "heartbeats must compact to their winning writes, kept {}",
            retained.len()
        );
        let mut alive_true_first = batches.clone();
        let len = alive_true_first.len();
        alive_true_first.swap(len - 6, len - 5);
        let removable = superseded_liveness_batches(&alive_true_first, &BTreeSet::new());
        let retained = alive_true_first
            .iter()
            .enumerate()
            .filter(|(index, _)| !removable.contains(index))
            .map(|(_, batch)| batch.clone())
            .collect::<Vec<_>>();
        assert_eq!(fold(&retained), fold(&alive_true_first));
    }
}
