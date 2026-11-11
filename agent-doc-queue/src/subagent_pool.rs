//! Subagent work pool (`#subagentworkpool`).
//!
//! A `subagents` / `fan-out` queue's workers are a pool, not a one-shot
//! batch. The pool has a size (the queue's `=N` cap, or a default derived
//! from the machine), and every event that frees a slot is a refill trigger:
//! a worker report (`queue report-complete`), a release or requeue, a lost
//! attempt, and a frozen worker. At each trigger the binary hands the
//! coordinator the next head's exact claim / brief / start commands, so the
//! refill happens in the same turn instead of waiting for another operator
//! prompt (live miss, 2026-11-05: 8 of 25 heads dispatched, the other 17 went
//! out only when the operator re-ran `/agent-doc`).
//!
//! Slot accounting, for one pool:
//!
//! * a `Claimed` attempt with no worker (dispatching) occupies a slot: the
//!   head is reserved, and the coordinator owes its spawn + `queue start`
//!   before anything else (the Grok sdk-dev miss, 2026-11-05: four claimed
//!   heads sat unspawned behind serial integration of sixteen reported rows,
//!   so the pool ran one worker);
//! * a `Running` attempt occupies a slot unless it is frozen;
//! * an attempt that expired with its worker still bound occupies a slot:
//!   expiry only means nobody refreshed the claim, so it is not evidence the
//!   worker stopped (live miss, 2026-11-05: three working Claude Code workers
//!   went `lost` at the two-hour TTL because nothing refreshed them);
//! * a frozen worker frees its slot but keeps its claim. Frozen means evidence
//!   the worker stopped: a recorded stop (`queue freeze`), or no heartbeat past
//!   [`DEFAULT_WORKER_FROZEN_SECS`] on an attempt this recovery does not own.
//!   work is proof of life while that wrapper remains open, so the coordinator
//!   records it as a heartbeat (`queue claim --refresh`). If the wrapper exits
//!   with that notice as its handback, the subagent-end hook freezes the ended
//!   wrapper and a continuation wrapper rebinds the same generation with
//!   `queue start`. A harness freeze with no handback stays `Running` and is not
//!   marked `Lost`. Continue refuses because that worker is gone, which frees the
//!   slot and offers the row for a new generation (`#lostrunningcontinue`). An
//!   exact refresh or continuation rebind proves the same worker is still alive,
//!   clears that failed continue, and keeps the generation
//!   (`#frozenclaimrefreshrevive`). A lost-running
//!   attempt is continued first. A heartbeat already past the frozen bound
//!   fails that continue, and the same task is retried into the freed slot
//!   (`#lostrunningcontinue`). Only `queue release --requeue` abandons it;
//! * `Reported` attempts, unbound lapsed attempts, and attempts abandoned with
//!   `queue release --requeue` hold no slot.
//!
//! A running worker's heartbeat falls due at half its claim window, well
//! before expiry, and every refill lists it with its exact refresh command.
//! A refill only reaches a coordinator through a boundary (a trigger command,
//! preflight, `plan`, or a tool call's PostToolUse), so a coordinator parked on
//! background work with no tool call running would never see one
//! (`#poolidleheartbeat`, 2026-11-05: 12 live workers' claims expired twice).
//! [`pool_heartbeat_wake`] is what the project controller's tick derives for
//! that coordinator. The tick only delivers the wake: a claim is refreshed by
//! the coordinator, which alone has evidence the worker is alive, so an
//! unattended refresh could never hide a worker that stopped.
//!
//! This module is pure policy. The machine probe (cores, build slots, env
//! override) lives in `agent-doc-queue-io::subagent_pool`, which feeds
//! [`PoolSizeInputs`]. The build-slot gate (`#buildconcurrencygate`) plugs in
//! through [`PoolSizeInputs::build_slots`]; this module never gates builds.

use std::num::NonZeroUsize;

use serde::{Deserialize, Serialize};

use crate::queue_claim::{QueueClaim, QueueClaimState};
use crate::subagent_intent::QueueSubagentsCapacity;

/// Lower bound of the derived default pool size.
pub const DEFAULT_SUBAGENT_POOL_FLOOR: usize = 8;
/// Upper bound of the derived default pool size.
pub const DEFAULT_SUBAGENT_POOL_CEILING: usize = 16;
/// Cores per pool slot when no build-slot count is known.
pub const CORES_PER_POOL_SLOT: usize = 4;
/// Pool slots per build slot when the build gate reports its slot count:
/// a worker spends most of its time reading, editing, and waiting, not in
/// heavy verification, so the pool runs ahead of the build gate.
pub const POOL_SLOTS_PER_BUILD_SLOT: usize = 2;

/// A bound worker with no heartbeat for this long is frozen: its pool slot is
/// freed for a refill while its claim is kept. This is deliberately longer than
/// the default claim TTL, so expiry alone can never read as a stopped worker.
pub const DEFAULT_WORKER_FROZEN_SECS: u64 =
    crate::queue_claim::DEFAULT_QUEUE_CLAIM_TTL_SECS * 3 / 2;

const _: () = assert!(
    DEFAULT_WORKER_FROZEN_SECS > crate::queue_claim::DEFAULT_QUEUE_CLAIM_TTL_SECS,
    "expiry must come before the frozen deadline"
);

/// Inputs the pool default is derived from. Every field is injected so the
/// policy is deterministic under test; the IO adapter probes the machine.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PoolSizeInputs {
    /// Operator override (`AGENT_DOC_SUBAGENT_POOL_SIZE`).
    pub override_size: Option<NonZeroUsize>,
    /// Concurrent heavy-verification slots reported by the build gate
    /// (`#buildconcurrencygate`). `None` until that gate is available.
    pub build_slots: Option<NonZeroUsize>,
    /// Available CPU parallelism.
    pub cores: Option<NonZeroUsize>,
}

/// Where a resolved pool size came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolSizeSource {
    /// The queue marker's explicit `subagents=N`.
    QueueCap,
    /// `AGENT_DOC_SUBAGENT_POOL_SIZE`.
    Override,
    /// Derived from the build gate's slot count.
    BuildSlots,
    /// Derived from CPU cores.
    Cores,
    /// No machine signal was available.
    Fallback,
}

impl PoolSizeSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::QueueCap => "queue_cap",
            Self::Override => "override",
            Self::BuildSlots => "build_slots",
            Self::Cores => "cores",
            Self::Fallback => "fallback",
        }
    }
}

/// A resolved pool size with its provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolSize {
    pub size: NonZeroUsize,
    pub source: PoolSizeSource,
}

fn clamp_default(size: usize) -> NonZeroUsize {
    let size = size.clamp(DEFAULT_SUBAGENT_POOL_FLOOR, DEFAULT_SUBAGENT_POOL_CEILING);
    NonZeroUsize::new(size).expect("pool floor is positive")
}

/// The default pool size for a bare `subagents` queue (no `=N`).
///
/// Precedence: operator override, then the build gate's slots
/// (`POOL_SLOTS_PER_BUILD_SLOT` per slot), then cores
/// (`CORES_PER_POOL_SLOT` per slot). Derived values are clamped to
/// `[DEFAULT_SUBAGENT_POOL_FLOOR, DEFAULT_SUBAGENT_POOL_CEILING]`; an
/// override is taken verbatim.
pub fn default_pool_size(inputs: PoolSizeInputs) -> PoolSize {
    if let Some(size) = inputs.override_size {
        return PoolSize {
            size,
            source: PoolSizeSource::Override,
        };
    }
    if let Some(slots) = inputs.build_slots {
        return PoolSize {
            size: clamp_default(slots.get().saturating_mul(POOL_SLOTS_PER_BUILD_SLOT)),
            source: PoolSizeSource::BuildSlots,
        };
    }
    if let Some(cores) = inputs.cores {
        return PoolSize {
            size: clamp_default(cores.get() / CORES_PER_POOL_SLOT),
            source: PoolSizeSource::Cores,
        };
    }
    PoolSize {
        size: clamp_default(DEFAULT_SUBAGENT_POOL_FLOOR),
        source: PoolSizeSource::Fallback,
    }
}

/// The pool size for one queue: its explicit `=N` cap, or the default.
pub fn resolve_pool_size(capacity: QueueSubagentsCapacity, default: PoolSize) -> PoolSize {
    match capacity {
        QueueSubagentsCapacity::Limited(size) => PoolSize {
            size,
            source: PoolSizeSource::QueueCap,
        },
        QueueSubagentsCapacity::AllEligible => default,
    }
}

/// Why a running worker no longer holds its pool slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum WorkerFrozen {
    /// `queue freeze`: the harness reported the worker stopped.
    HarnessReported { frozen_at_secs: u64, reason: String },
    /// No heartbeat (start or `queue claim --refresh`) within the bound.
    HeartbeatStale {
        last_heartbeat_secs: u64,
        stale_for_secs: u64,
    },
}

impl WorkerFrozen {
    pub fn describe(&self) -> String {
        match self {
            Self::HarnessReported { reason, .. } if !reason.is_empty() => {
                format!("harness reported stopped: {reason}")
            }
            Self::HarnessReported { .. } => "harness reported stopped".to_string(),
            Self::HeartbeatStale { stale_for_secs, .. } => {
                format!("no heartbeat for {stale_for_secs}s")
            }
        }
    }
}

/// The last heartbeat of a running attempt: its latest refresh, else when
/// its worker was bound.
pub fn last_heartbeat_secs(claim: &QueueClaim) -> Option<u64> {
    let started = claim.worker.as_ref().map(|worker| worker.started_at_secs);
    match (claim.heartbeat_at_secs, started) {
        (Some(beat), Some(started)) => Some(beat.max(started)),
        (beat, started) => beat.or(started),
    }
}

/// What one attempt means for its pool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttemptSlot {
    /// Holds no slot: reported, abandoned, or lapsed with no worker bound.
    Free,
    /// Claimed with no worker yet; holds a slot and owes its spawn.
    Dispatching,
    /// A live worker; holds a slot.
    Running,
    /// Expired with its worker still bound and no evidence it stopped; holds
    /// a slot and its row is not offered again.
    Unconfirmed,
    /// Evidence the worker stopped; holds no slot, keeps its claim.
    Frozen(WorkerFrozen),
}

/// Classify one attempt. Callers may pass an attempt whose expiry was already
/// projected to `Lost`, or one still `Running` past its expiry.
pub fn classify_attempt(claim: &QueueClaim, now_secs: u64, frozen_secs: u64) -> AttemptSlot {
    // `#harnessquotaexhausted`: an exhausted worker holds no slot and is not
    // frozen. Whether its harness may refill the slot is
    // `worker_quota::pool_refill_quota_block`.
    if matches!(
        claim.state,
        QueueClaimState::Reported | QueueClaimState::QuotaBlocked
    ) {
        return AttemptSlot::Free;
    }
    // A lost running attempt whose continue failed no longer holds a slot:
    // the replacement generation has to be able to start (`#lostrunningcontinue`).
    if crate::lost_running_recovery::retry_authorized(claim) {
        return AttemptSlot::Free;
    }
    let lapsed = claim.state == QueueClaimState::Lost || claim.is_expired(now_secs);
    if claim.worker.is_none() {
        return if lapsed || claim.state != QueueClaimState::Claimed {
            AttemptSlot::Free
        } else {
            AttemptSlot::Dispatching
        };
    }
    if let Some(freeze) = &claim.frozen {
        // An explicit requeue is recovery debt once the attempt has lapsed.
        // A harness freeze is not: the worker may resume through its
        // transcript, so expiry must not drop the claim
        // (`#frozenclaimrefreshrevive`).
        if freeze.is_abandon() && lapsed {
            return AttemptSlot::Free;
        }
        return AttemptSlot::Frozen(WorkerFrozen::HarnessReported {
            frozen_at_secs: freeze.frozen_at_secs,
            reason: freeze.reason.clone(),
        });
    }
    if let Some(last) = last_heartbeat_secs(claim) {
        let stale_for_secs = now_secs.saturating_sub(last);
        if stale_for_secs >= frozen_secs.max(1) {
            return AttemptSlot::Frozen(WorkerFrozen::HeartbeatStale {
                last_heartbeat_secs: last,
                stale_for_secs,
            });
        }
    }
    if lapsed {
        AttemptSlot::Unconfirmed
    } else {
        AttemptSlot::Running
    }
}

/// Whether `claim` is a frozen worker.
pub fn worker_frozen(claim: &QueueClaim, now_secs: u64, frozen_secs: u64) -> Option<WorkerFrozen> {
    match classify_attempt(claim, now_secs, frozen_secs) {
        AttemptSlot::Frozen(frozen) => Some(frozen),
        _ => None,
    }
}

/// Whether `claim` holds a pool slot right now.
pub fn occupies_pool_slot(claim: &QueueClaim, now_secs: u64, frozen_secs: u64) -> bool {
    matches!(
        classify_attempt(claim, now_secs, frozen_secs),
        AttemptSlot::Dispatching | AttemptSlot::Running | AttemptSlot::Unconfirmed
    )
}

/// Whether a bound worker's heartbeat is due: half its claim window has passed
/// (well before expiry), or the attempt already lapsed.
pub fn heartbeat_due(claim: &QueueClaim, now_secs: u64, frozen_secs: u64) -> bool {
    match classify_attempt(claim, now_secs, frozen_secs) {
        AttemptSlot::Unconfirmed => true,
        AttemptSlot::Running => last_heartbeat_secs(claim).is_some_and(|last| {
            let window = claim.expires_at_secs.saturating_sub(last);
            now_secs.saturating_sub(last) >= window / 2
        }),
        _ => false,
    }
}

/// Seconds past the moment a bound worker's heartbeat fell due; zero before it.
pub fn heartbeat_overdue_secs(claim: &QueueClaim, now_secs: u64) -> u64 {
    last_heartbeat_secs(claim).map_or(0, |last| {
        let window = claim.expires_at_secs.saturating_sub(last);
        now_secs.saturating_sub(last).saturating_sub(window / 2)
    })
}

/// Whether any attempt in `claims` could have a due heartbeat, from the claim
/// ledger alone. The project controller's tick asks this before it reads a
/// document, so a quiescent document costs a ledger read and no text. It
/// ignores the frozen bound on purpose: the exact answer needs the document's
/// live rows anyway, and a stale worker must not be hidden from that answer.
pub fn heartbeat_may_be_due<'a>(
    claims: impl IntoIterator<Item = &'a QueueClaim>,
    now_secs: u64,
) -> bool {
    claims
        .into_iter()
        .any(|claim| heartbeat_due(claim, now_secs, u64::MAX))
}

/// How long a due heartbeat waits for its siblings before an idle coordinator
/// is woken for it. Workers spawned minutes apart fall due minutes apart, and
/// each wake costs a coordinator turn, so one wake carries the whole batch.
/// Well inside the half claim window between due and expiry, and an expired
/// claim never waits.
pub const POOL_HEARTBEAT_WAKE_COALESCE_SECS: u64 = 300;

/// What the project controller's tick hands an idle coordinator: one wake for
/// one distinct set of due heartbeats.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolHeartbeatWake {
    /// Identity of the due set. A refresh moves `last_heartbeat_secs` and an
    /// expiry flips `expired`, so either is a new set and a fresh wake; an
    /// unchanged set is delivered once.
    pub fingerprint: String,
    pub due: usize,
    pub expired: usize,
}

/// The wake `refresh_due` calls for, or `None` while nothing is due or the due
/// set is still inside [`POOL_HEARTBEAT_WAKE_COALESCE_SECS`].
pub fn pool_heartbeat_wake(refresh_due: &[RefreshDue]) -> Option<PoolHeartbeatWake> {
    let expired = refresh_due.iter().filter(|entry| entry.expired).count();
    let held_past_coalesce = refresh_due
        .iter()
        .any(|entry| entry.overdue_secs >= POOL_HEARTBEAT_WAKE_COALESCE_SECS);
    if expired == 0 && !held_past_coalesce {
        return None;
    }
    let mut parts = refresh_due
        .iter()
        .map(|entry| {
            format!(
                "{}:{}:{}:{}",
                entry.generation, entry.last_heartbeat_secs, entry.expired, entry.item
            )
        })
        .collect::<Vec<_>>();
    parts.sort();
    Some(PoolHeartbeatWake {
        fingerprint: agent_doc_hash::content_hash(&parts.join("\n")),
        due: refresh_due.len(),
        expired,
    })
}

/// Slot accounting for one pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolOccupancy {
    pub size: usize,
    pub source: PoolSizeSource,
    /// Running workers that hold a slot.
    pub running: usize,
    /// Claimed heads with no worker bound yet (dispatching); they hold a slot.
    pub dispatching: usize,
    /// Attempts that expired with their worker still bound; they hold a slot.
    pub unconfirmed: usize,
    /// Workers with evidence they stopped; they hold no slot.
    pub frozen: usize,
    /// Slots a refill may fill now.
    pub free: usize,
}

/// Account `claims` (the pool's live attempts) against `size`.
pub fn pool_occupancy<'a>(
    size: PoolSize,
    claims: impl IntoIterator<Item = &'a QueueClaim>,
    now_secs: u64,
    frozen_secs: u64,
) -> PoolOccupancy {
    let mut running = 0;
    let mut dispatching = 0;
    let mut unconfirmed = 0;
    let mut frozen = 0;
    for claim in claims {
        match classify_attempt(claim, now_secs, frozen_secs) {
            AttemptSlot::Free => {}
            AttemptSlot::Dispatching => dispatching += 1,
            AttemptSlot::Running => running += 1,
            AttemptSlot::Unconfirmed => unconfirmed += 1,
            AttemptSlot::Frozen(_) => frozen += 1,
        }
    }
    PoolOccupancy {
        size: size.size.get(),
        source: size.source,
        running,
        dispatching,
        unconfirmed,
        frozen,
        free: size
            .size
            .get()
            .saturating_sub(running + dispatching + unconfirmed),
    }
}

/// The event that freed (or may have freed) a pool slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolRefillTrigger {
    ReportComplete,
    Release,
    Lost,
    Frozen,
    /// A worker stopped on a harness usage limit (`#workerquotaautodetect`).
    /// The attempt holds no pool slot.
    QuotaBlocked,
    /// A planning boundary (preflight, `plan`, steering, `queue pool`).
    Plan,
    /// Heads were added, or claims are still unstarted, while the owner turn
    /// is running. Hookless harnesses receive this as a composer follow-up
    /// (`#subagentworkpool`).
    RunningTurn,
}

impl PoolRefillTrigger {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReportComplete => "report_complete",
            Self::Release => "release",
            Self::Lost => "lost",
            Self::Frozen => "frozen",
            Self::QuotaBlocked => "quota_blocked",
            Self::Plan => "plan",
            Self::RunningTurn => "running_turn",
        }
    }
}

/// One claimed head with no worker bound: urgent dispatch debt that comes
/// before any reported-result integration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnstartedDispatch {
    pub item: String,
    pub owner: String,
    pub generation: u64,
    pub claimed_for_secs: u64,
    pub brief_command: String,
    pub start_command: String,
    pub release_command: String,
}

/// One frozen worker whose slot was freed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrozenWorker {
    pub item: String,
    pub owner: String,
    pub generation: u64,
    pub worker_id: String,
    pub frozen: WorkerFrozen,
}

/// A bound worker whose heartbeat is due. The coordinator confirms it is alive
/// (a harness "waiting on its own background work" notice counts) and refreshes
/// it, or records that it stopped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefreshDue {
    pub item: String,
    pub owner: String,
    pub generation: u64,
    pub worker_id: String,
    pub last_heartbeat_secs: u64,
    pub heartbeat_age_secs: u64,
    /// Seconds past the moment this heartbeat fell due.
    #[serde(default)]
    pub overdue_secs: u64,
    /// The claim already expired; the attempt is unconfirmed, not freed.
    pub expired: bool,
    pub refresh_command: String,
    pub freeze_command: String,
}

/// The next head a refill dispatches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefillHead {
    pub item: String,
    pub claim_command: String,
    pub brief_command: String,
    pub start_command: String,
}

/// Why a refill withheld `next_heads` (`#poolquotarefillgate`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuotaRefillBlock {
    /// Canonical harness the refill refused. `None` when the caller named
    /// none: the gate then refuses rather than guessing a different harness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
    /// [`crate::worker_quota::quota_blocked_next_step`] for the blocking attempt.
    pub reason: String,
}

/// One pool's status line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolStatus {
    pub queue_id: String,
    pub occupancy: PoolOccupancy,
}

/// The claim/start text a live hookless turn is pasted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolRunningTurnRefill {
    /// Unstarted claims and next heads only. A heartbeat change is not a new paste.
    pub fingerprint: String,
    pub text: String,
    pub next_heads: usize,
    pub unstarted: usize,
}

/// Everything a refill trigger hands the coordinator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolRefill {
    pub trigger: PoolRefillTrigger,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pools: Vec<PoolStatus>,
    /// Claimed heads with no worker: spawn + start these FIRST.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unstarted: Vec<UnstartedDispatch>,
    /// Bound workers whose heartbeat is due: refresh them before they expire.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refresh_due: Vec<RefreshDue>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frozen: Vec<FrozenWorker>,
    /// The next heads to dispatch now, already bounded by free slots.
    /// Empty when [`Self::quota_refill_block`] withheld them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_heads: Vec<RefillHead>,
    /// Set when a harness usage limit withholds `next_heads`
    /// (`#poolquotarefillgate`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota_refill_block: Option<QuotaRefillBlock>,
    /// Heads still waiting for a slot after this refill.
    pub waiting_for_slot: usize,
}

impl PoolRefill {
    /// Identity of the dispatch debt a running-turn paste must not repeat.
    pub fn dispatch_fingerprint(&self) -> String {
        let mut parts = Vec::new();
        for entry in &self.unstarted {
            parts.push(format!("u:{}:{}", entry.generation, entry.item));
        }
        for head in &self.next_heads {
            parts.push(format!("n:{}", head.item));
        }
        agent_doc_hash::content_hash(&parts.join("\n"))
    }

    /// Claim/start steps for a turn that is already running, when this refill
    /// has dispatch debt. Heartbeats and quota refusals stay on their own paths.
    pub fn running_turn_refill(&self) -> Option<PoolRunningTurnRefill> {
        if self.unstarted.is_empty() && self.next_heads.is_empty() {
            return None;
        }
        let text = self.render()?;
        Some(PoolRunningTurnRefill {
            fingerprint: self.dispatch_fingerprint(),
            next_heads: self.next_heads.len(),
            unstarted: self.unstarted.len(),
            text,
        })
    }

    /// Whether the coordinator has pool work to do now. A quota refusal is
    /// actionable so the reason is surfaced, not only the heads it removed.
    pub fn is_actionable(&self) -> bool {
        !self.unstarted.is_empty()
            || !self.refresh_due.is_empty()
            || !self.next_heads.is_empty()
            || self.quota_refill_block.is_some()
    }

    /// One line per due heartbeat.
    pub fn refresh_due_steps(&self) -> Vec<String> {
        self.refresh_due
            .iter()
            .map(|entry| {
                format!(
                    "Heartbeat due: queue item {:?} (worker {}, generation {}) has no heartbeat \
                     for {}s{}. If the worker is alive (a harness \"waiting on its own background \
                     work\" notice is proof of life), run `{}`. Only if the harness reported it \
                     ended without a result, run `{}`. Expiry alone neither frees its slot nor \
                     offers its row again.",
                    entry.item,
                    entry.worker_id,
                    entry.generation,
                    entry.heartbeat_age_secs,
                    if entry.expired {
                        " and its claim expired"
                    } else {
                        ""
                    },
                    entry.refresh_command,
                    entry.freeze_command,
                )
            })
            .collect()
    }

    /// Ordered commands: unstarted spawn/start first, then due heartbeats,
    /// then refill heads.
    pub fn ordered_steps(&self) -> Vec<String> {
        let mut steps = Vec::new();
        for entry in &self.unstarted {
            steps.push(format!(
                "URGENT, before any reported-result integration: queue item {:?} is claimed by {} \
                 at generation {} with no worker bound ({}s). Spawn its worker now with the \
                 preamble from `{}`, then immediately run `{}` with the returned worker id. Only \
                 if it cannot be spawned, release it for redispatch: `{}`.",
                entry.item,
                entry.owner,
                entry.generation,
                entry.claimed_for_secs,
                entry.brief_command,
                entry.start_command,
                entry.release_command,
            ));
        }
        steps.extend(self.refresh_due_steps());
        for head in &self.next_heads {
            steps.push(format!(
                "Refill a free pool slot with queue item {:?}: run `{}`; after it succeeds spawn \
                 one background worker with the preamble from `{}`; then immediately run `{}` with \
                 the printed generation and the returned worker id.",
                head.item, head.claim_command, head.brief_command, head.start_command,
            ));
        }
        steps
    }

    /// Text for a command's output or a steering context. `None` when there
    /// is neither pool work nor a pool to report.
    pub fn render(&self) -> Option<String> {
        if self.pools.is_empty() && !self.is_actionable() && self.frozen.is_empty() {
            return None;
        }
        let mut out = format!(
            "[queue] subagent work pool (#subagentworkpool, trigger={}):",
            self.trigger.as_str()
        );
        for pool in &self.pools {
            let o = pool.occupancy;
            out.push_str(&format!(
                "\n  pool `{}`: size {} ({}), running {}, dispatching {}, unconfirmed {}, frozen {}, free {}",
                pool.queue_id,
                o.size,
                o.source.as_str(),
                o.running,
                o.dispatching,
                o.unconfirmed,
                o.frozen,
                o.free,
            ));
        }
        for worker in &self.frozen {
            out.push_str(&format!(
                "\n  frozen: {:?} (worker {}, generation {}): {}; its slot is free and its claim is kept. `queue claim --refresh` with this owner and generation revives a transcript resume, including after the claim TTL; the head is not eligible for dispatch",
                worker.item,
                worker.worker_id,
                worker.generation,
                worker.frozen.describe(),
            ));
        }
        if let Some(block) = &self.quota_refill_block {
            let on = block
                .harness
                .as_deref()
                .map(|harness| format!(" on {harness}"))
                .unwrap_or_default();
            out.push_str(&format!(
                "\n  quota: next heads withheld ({}{on}): {}",
                crate::worker_quota::WORKER_QUOTA_EXHAUSTED_REASON,
                block.reason,
            ));
        }
        let steps = self.ordered_steps();
        if steps.is_empty() {
            if self.quota_refill_block.is_some() {
                out.push_str("\n  no refill head is offered while that harness is out of usage.");
                if self.waiting_for_slot > 0 {
                    out.push_str(&format!(
                        "\n  {} more head(s) stay held.",
                        self.waiting_for_slot
                    ));
                }
            } else if self.waiting_for_slot > 0 {
                out.push_str(&format!(
                    "\n  no free slot; {} head(s) wait. Refill on the next report-complete, release, lost, or frozen worker.",
                    self.waiting_for_slot
                ));
            } else {
                out.push_str("\n  no head waits for a slot.");
            }
            return Some(out);
        }
        out.push_str(
            "\n  Do these NOW, in this order, in this turn (spawn and start every claimed head before integrating any reported result):",
        );
        for (index, step) in steps.iter().enumerate() {
            out.push_str(&format!("\n  {}. {step}", index + 1));
        }
        if self.waiting_for_slot > 0 {
            out.push_str(&format!(
                "\n  {} more head(s) wait for a slot; the next trigger hands them out.",
                self.waiting_for_slot
            ));
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue_claim::{QueueWorkerBinding, QueueWorkerFreeze, claim_identity};

    fn nz(n: usize) -> NonZeroUsize {
        NonZeroUsize::new(n).unwrap()
    }

    fn claim(item: &str, state: QueueClaimState, worker_started: Option<u64>) -> QueueClaim {
        QueueClaim {
            identity: claim_identity(item),
            item_text: item.to_string(),
            owner: format!("subagent:{item}"),
            generation: 1,
            state,
            lost_from: None,
            completion_recovered_after_loss: false,
            publication_state: None,
            worker: worker_started.map(|started_at_secs| QueueWorkerBinding {
                worker_id: format!("w-{item}"),
                started_at_secs,
            }),
            dispatch_policy: None,
            tracked_item_aliases: Vec::new(),
            claimed_at_secs: 0,
            expires_at_secs: 1_000_000,
            owner_session: None,
            anchor: None,
            followed_from: Vec::new(),
            followed_generation: 0,
            claim_time_row: None,
            reported_at_secs: None,
            heartbeat_at_secs: None,
            frozen: None,
            quota_block: None,
            lost_continue: None,
        }
    }

    #[test]
    fn default_pool_size_precedence_and_clamp() {
        let base = PoolSizeInputs::default();
        assert_eq!(
            default_pool_size(base),
            PoolSize {
                size: nz(8),
                source: PoolSizeSource::Fallback
            }
        );
        let cores = PoolSizeInputs {
            cores: Some(nz(32)),
            ..base
        };
        assert_eq!(default_pool_size(cores).size, nz(8));
        assert_eq!(default_pool_size(cores).source, PoolSizeSource::Cores);
        let big = PoolSizeInputs {
            cores: Some(nz(128)),
            ..base
        };
        assert_eq!(
            default_pool_size(big).size,
            nz(16),
            "clamped to the ceiling"
        );
        let small = PoolSizeInputs {
            cores: Some(nz(2)),
            ..base
        };
        assert_eq!(default_pool_size(small).size, nz(8), "clamped to the floor");
        let slots = PoolSizeInputs {
            build_slots: Some(nz(6)),
            ..cores
        };
        assert_eq!(
            default_pool_size(slots),
            PoolSize {
                size: nz(12),
                source: PoolSizeSource::BuildSlots
            },
            "the build gate seam outranks cores"
        );
        let over = PoolSizeInputs {
            override_size: Some(nz(3)),
            ..slots
        };
        assert_eq!(
            default_pool_size(over),
            PoolSize {
                size: nz(3),
                source: PoolSizeSource::Override
            },
            "an operator override is verbatim, never clamped"
        );
    }

    #[test]
    fn an_explicit_cap_is_the_pool_size() {
        let default = default_pool_size(PoolSizeInputs::default());
        assert_eq!(
            resolve_pool_size(QueueSubagentsCapacity::Limited(nz(3)), default),
            PoolSize {
                size: nz(3),
                source: PoolSizeSource::QueueCap
            }
        );
        assert_eq!(
            resolve_pool_size(QueueSubagentsCapacity::AllEligible, default),
            default
        );
    }

    #[test]
    fn frozen_workers_free_their_slot_but_dispatching_claims_hold_one() {
        let now = 10_000;
        let frozen_secs = 600;
        let live = claim("live", QueueClaimState::Running, Some(now - 10));
        let stale = claim("stale", QueueClaimState::Running, Some(now - 700));
        let mut beat = claim("beat", QueueClaimState::Running, Some(now - 700));
        beat.heartbeat_at_secs = Some(now - 5);
        let mut stopped = claim("stopped", QueueClaimState::Running, Some(now - 1));
        stopped.frozen = Some(QueueWorkerFreeze {
            frozen_at_secs: now,
            reason: "ended without a result".to_string(),
            abandoned: false,
        });
        let dispatching = claim("dispatching", QueueClaimState::Claimed, None);
        let reported = claim("reported", QueueClaimState::Reported, Some(1));
        let lost = claim("lost", QueueClaimState::Lost, None);

        assert!(worker_frozen(&live, now, frozen_secs).is_none());
        assert!(matches!(
            worker_frozen(&stale, now, frozen_secs),
            Some(WorkerFrozen::HeartbeatStale {
                stale_for_secs: 700,
                ..
            })
        ));
        assert!(
            worker_frozen(&beat, now, frozen_secs).is_none(),
            "a refresh is a heartbeat"
        );
        assert!(matches!(
            worker_frozen(&stopped, now, frozen_secs),
            Some(WorkerFrozen::HarnessReported { .. })
        ));
        assert!(worker_frozen(&dispatching, now, frozen_secs).is_none());

        let occupancy = pool_occupancy(
            PoolSize {
                size: nz(4),
                source: PoolSizeSource::QueueCap,
            },
            [
                &live,
                &stale,
                &beat,
                &stopped,
                &dispatching,
                &reported,
                &lost,
            ],
            now,
            frozen_secs,
        );
        assert_eq!(occupancy.running, 2, "{occupancy:?}");
        assert_eq!(occupancy.dispatching, 1, "{occupancy:?}");
        assert_eq!(occupancy.frozen, 2, "{occupancy:?}");
        assert_eq!(occupancy.free, 1, "{occupancy:?}");
        assert!(occupies_pool_slot(&dispatching, now, frozen_secs));
        assert!(!occupies_pool_slot(&stale, now, frozen_secs));
        assert!(!occupies_pool_slot(&reported, now, frozen_secs));
    }

    #[test]
    fn refill_orders_unstarted_spawns_before_new_heads() {
        let refill = PoolRefill {
            trigger: PoolRefillTrigger::ReportComplete,
            pools: Vec::new(),
            unstarted: vec![UnstartedDispatch {
                item: "do [#a]".to_string(),
                owner: "subagent:a".to_string(),
                generation: 2,
                claimed_for_secs: 400,
                brief_command: "agent-doc queue brief f --item a".to_string(),
                start_command: "agent-doc queue start f --item a --generation 2".to_string(),
                release_command: "agent-doc queue release f --item a --requeue".to_string(),
            }],
            refresh_due: vec![RefreshDue {
                item: "do [#r]".to_string(),
                owner: "subagent:r".to_string(),
                generation: 1,
                worker_id: "w".to_string(),
                last_heartbeat_secs: 0,
                heartbeat_age_secs: 3700,
                overdue_secs: 100,
                expired: false,
                refresh_command: "agent-doc queue claim f --item r --refresh".to_string(),
                freeze_command: "agent-doc queue freeze f --item r".to_string(),
            }],
            frozen: Vec::new(),
            quota_refill_block: None,
            next_heads: vec![RefillHead {
                item: "do [#b]".to_string(),
                claim_command: "agent-doc queue claim f --item b".to_string(),
                brief_command: "agent-doc queue brief f --item b".to_string(),
                start_command: "agent-doc queue start f --item b".to_string(),
            }],
            waiting_for_slot: 3,
        };
        let steps = refill.ordered_steps();
        assert_eq!(steps.len(), 3);
        assert!(steps[0].starts_with("URGENT"), "{steps:?}");
        assert!(steps[1].starts_with("Heartbeat due"), "{steps:?}");
        assert!(steps[1].contains("proof of life"), "{steps:?}");
        assert!(steps[0].contains("queue start f --item a --generation 2"));
        assert!(steps[2].contains("queue claim f --item b"));
        let text = refill.render().unwrap();
        assert!(text.contains("trigger=report_complete"), "{text}");
        assert!(
            text.contains("before integrating any reported result"),
            "{text}"
        );
        assert!(text.contains("3 more head(s) wait"), "{text}");
    }

    /// `#poolquotarefillgate`: a refusal is the report, not an empty pool.
    #[test]
    fn a_quota_refusal_names_the_reason_and_offers_no_head() {
        let refill = PoolRefill {
            trigger: PoolRefillTrigger::ReportComplete,
            pools: Vec::new(),
            unstarted: Vec::new(),
            refresh_due: Vec::new(),
            frozen: Vec::new(),
            next_heads: Vec::new(),
            quota_refill_block: Some(QuotaRefillBlock {
                harness: Some("claude-code".to_string()),
                reason: "do not start another worker on claude-code".to_string(),
            }),
            waiting_for_slot: 2,
        };
        assert!(refill.is_actionable());
        let text = refill.render().unwrap();
        assert!(
            text.contains("next heads withheld (worker_quota_exhausted on claude-code)"),
            "{text}"
        );
        assert!(
            text.contains("do not start another worker on claude-code"),
            "{text}"
        );
        assert!(text.contains("no refill head is offered"), "{text}");
        assert!(text.contains("2 more head(s) stay held"), "{text}");
        assert!(!text.contains("no head waits for a slot"), "{text}");
        assert!(!text.contains("queue claim"), "{text}");
    }

    /// Expiry alone is not evidence the worker stopped: an expired bound
    /// attempt keeps its slot until a recorded stop or the frozen deadline.
    #[test]
    fn an_expired_bound_attempt_holds_its_slot_until_evidence_it_stopped() {
        let frozen_secs = 1_000;
        let mut claim = claim("worker", QueueClaimState::Running, Some(100));
        claim.expires_at_secs = 500;
        let expired_at = 600;
        assert_eq!(
            classify_attempt(&claim, expired_at, frozen_secs),
            AttemptSlot::Unconfirmed
        );
        claim.mark_lost();
        assert_eq!(
            classify_attempt(&claim, expired_at, frozen_secs),
            AttemptSlot::Unconfirmed
        );
        assert!(occupies_pool_slot(&claim, expired_at, frozen_secs));
        assert!(heartbeat_due(&claim, expired_at, frozen_secs));
        assert!(matches!(
            classify_attempt(&claim, 1_100, frozen_secs),
            AttemptSlot::Frozen(WorkerFrozen::HeartbeatStale { .. })
        ));
        claim.frozen = Some(QueueWorkerFreeze {
            frozen_at_secs: 600,
            reason: crate::queue_claim::ABANDONED_REQUEUE_REASON.to_string(),
            abandoned: true,
        });
        assert_eq!(
            classify_attempt(&claim, expired_at, frozen_secs),
            AttemptSlot::Free
        );
        claim.frozen = Some(QueueWorkerFreeze {
            frozen_at_secs: 600,
            reason: "ended without a result".to_string(),
            abandoned: false,
        });
        assert!(
            matches!(
                classify_attempt(&claim, expired_at, frozen_secs),
                AttemptSlot::Frozen(WorkerFrozen::HarnessReported { .. })
            ),
            "a harness freeze stays frozen after the TTL"
        );
        assert!(!occupies_pool_slot(&claim, expired_at, frozen_secs));
        assert!(claim.blocks_dispatch(expired_at));
    }

    /// A heartbeat falls due halfway through the claim window, not at expiry.
    #[test]
    fn a_heartbeat_falls_due_well_before_expiry() {
        let mut claim = claim("worker", QueueClaimState::Running, Some(1_000));
        claim.expires_at_secs = 8_200;
        assert!(!heartbeat_due(&claim, 4_599, u64::MAX));
        assert!(heartbeat_due(&claim, 4_600, u64::MAX));
        claim.heartbeat_at_secs = Some(4_600);
        claim.expires_at_secs = 11_800;
        assert!(
            !heartbeat_due(&claim, 4_601, u64::MAX),
            "a refresh restarts the window"
        );
    }

    fn due(item: &str, overdue_secs: u64, expired: bool) -> RefreshDue {
        RefreshDue {
            item: item.to_string(),
            owner: format!("subagent:{item}"),
            generation: 1,
            worker_id: format!("w-{item}"),
            last_heartbeat_secs: 100,
            heartbeat_age_secs: 3_600 + overdue_secs,
            overdue_secs,
            expired,
            refresh_command: String::new(),
            freeze_command: String::new(),
        }
    }

    /// `#poolidleheartbeat`: workers spawned minutes apart coalesce into one
    /// wake, the oldest one sets the deadline, and expiry never waits.
    #[test]
    fn a_heartbeat_wake_holds_for_its_siblings_until_the_oldest_is_past_the_window() {
        assert_eq!(pool_heartbeat_wake(&[]), None);
        let just_due = [due("a", 0, false), due("b", 60, false)];
        assert_eq!(pool_heartbeat_wake(&just_due), None);

        let oldest_past = [
            due("a", POOL_HEARTBEAT_WAKE_COALESCE_SECS, false),
            due("b", 60, false),
        ];
        let wake = pool_heartbeat_wake(&oldest_past).expect("the batch wakes");
        assert_eq!((wake.due, wake.expired), (2, 0));

        let expired = pool_heartbeat_wake(&[due("a", 0, true)]).expect("expiry never waits");
        assert_eq!((expired.due, expired.expired), (1, 1));
    }

    /// One wake per distinct due set: order is not identity, a refresh and an
    /// expiry are.
    #[test]
    fn a_heartbeat_wake_fingerprint_follows_the_due_set() {
        let aged = POOL_HEARTBEAT_WAKE_COALESCE_SECS;
        let fingerprint =
            |entries: &[RefreshDue]| pool_heartbeat_wake(entries).unwrap().fingerprint;
        let base = fingerprint(&[due("a", aged, false), due("b", aged, false)]);
        assert_eq!(
            base,
            fingerprint(&[due("b", aged + 60, false), due("a", aged + 60, false)]),
            "a later tick over the same set is the same wake"
        );
        assert_ne!(
            base,
            fingerprint(&[due("a", aged, false)]),
            "a partial refresh"
        );
        assert_ne!(
            base,
            fingerprint(&[due("a", aged, true), due("b", aged, false)]),
            "an expiry"
        );
        let mut refreshed = due("a", aged, false);
        refreshed.last_heartbeat_secs = 3_700;
        assert_ne!(
            base,
            fingerprint(&[refreshed, due("b", aged, false)]),
            "a refresh"
        );
    }

    #[test]
    fn overdue_counts_from_half_the_claim_window() {
        let mut running = claim("do [#a]", QueueClaimState::Running, Some(1_000));
        running.expires_at_secs = 8_200;
        assert_eq!(heartbeat_overdue_secs(&running, 4_599), 0);
        assert_eq!(heartbeat_overdue_secs(&running, 4_600), 0);
        assert_eq!(heartbeat_overdue_secs(&running, 4_900), 300);
        assert!(heartbeat_due(&running, 4_600, DEFAULT_WORKER_FROZEN_SECS));
    }

    /// The ledger gate may only ever say "maybe": a worker the exact projection
    /// calls due must pass it, frozen-stale workers included.
    #[test]
    fn the_ledger_gate_never_hides_a_due_heartbeat() {
        let mut running = claim("do [#a]", QueueClaimState::Running, Some(1_000));
        running.expires_at_secs = 8_200;
        let claims = [running];
        assert!(!heartbeat_may_be_due(&claims, 4_599));
        for now in [4_600, 8_200, 8_201, 1_000_000] {
            assert!(heartbeat_may_be_due(&claims, now), "{now}");
        }
        let unbound = [claim("do [#b]", QueueClaimState::Claimed, None)];
        assert!(!heartbeat_may_be_due(&unbound, 1_000_000));
    }
}
