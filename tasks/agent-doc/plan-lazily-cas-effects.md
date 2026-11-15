# Plan: Lazily-owned CAS effects

## Goal

Move effect compare-and-swap orchestration out of agent-doc and into Lazily. Lazily
will own effect identity, expected authority/version, keyed single-flight,
latest-wins coalescing, supersession, retry state, and terminal receipts. Agent-doc
will supply narrow backend adapters that atomically compare and mutate their own
authority boundary.

The first consumer is deferred queue maintenance, replacing
`PendingMaintenanceEffectFence` and its repeated per-family checks. The same
primitive should then replace the other bespoke effect-CAS state machines in
agent-doc where their semantics match.

## Current substrate

- `lazily-spec/docs/durable-sinks.md` already assigns live coordination and CAS
  to Lazily state and defines the keyed, single-flight
  `LatestDurableProjectionCore` protocol.
- `lazily-rs/src/latest_durable_projection_core.rs` already implements most of
  the required algebra: generations, immutable epochs, latest-wins pending
  intent, one in-flight value per key, stale-receipt rejection, retry,
  supersession, reconnect, and monotone acknowledgement.
- Lazily durable-owner types already provide stable effect and receipt
  identities, but they target durable owner transactions rather than generic
  live effect execution.
- agent-doc currently duplicates the missing protocol across
  `PendingMaintenanceEffectFence`, `QueueMaintenanceEffectFamily`, background
  request leases, Git transaction fences, document-authority hashes, controller
  and ledger writes, snapshot/worktree effects, and ops evidence.

The implementation should generalize and compose the existing Lazily primitives,
not introduce a parallel queue or another application-local retry loop.

## Required semantics

### Ownership boundary

Lazily owns:

- stable `EffectIdentity` and immutable `EffectIntent` identity;
- the observed authority token/version used by the attempt;
- keyed single-flight claims and actor generations;
- latest-wins coalescing and explicit supersession;
- retry eligibility/backoff inputs, reconnect, and crash-resumable retained
  intent;
- exact terminal receipt matching and monotone applied frontiers;
- the sequence cursor for a multi-step effect plan.

The application adapter owns:

- reading a backend's current opaque authority token without side effects;
- one atomic `compare_and_apply(expected, payload)` operation;
- mapping backend outcomes into `Applied`, `Superseded { observed }`,
  `Retryable`, or `Rejected`;
- backend-specific durability and idempotency.

An adapter must not perform application-level retry or choose a newer intent.
A failed comparison returns `Superseded` to Lazily, which decides whether to
drop, coalesce, or schedule the current latest intent.

### Atomicity boundary

CAS is atomic only inside one adapter boundary: for example a Git `update-ref`,
an editor-authority transaction, or a SQLite transaction. Lazily cannot make
Git, editor state, SQLite, and filesystem cleanup one physical transaction.

For heterogeneous effect plans, Lazily therefore owns an ordered, receipted
sequence:

1. apply the canonical authority mutation first;
2. derive every later step from that applied receipt/version;
3. validate the receipt lineage before each step;
4. make later steps idempotent and resumable;
5. stop immediately on supersession, retaining only unapplied work.

This prevents stale work from continuing and makes partial progress recoverable;
it must not be described as cross-backend atomicity.

## Proposed Lazily API

Names are provisional; settle them in the spec before implementation.

```rust
pub struct EffectKey<K> {
    pub scope: K,
    pub family: EffectFamily,
}

pub struct EffectIntent<K, A, P> {
    pub identity: EffectIdentity,
    pub generation: u64,
    pub epoch: u64,
    pub key: EffectKey<K>,
    pub expected: A,
    pub payload: P,
}

pub enum EffectApply<R, A, E> {
    Applied { receipt: R, authority: A },
    Superseded { observed: A },
    Retryable(E),
    Rejected(E),
}

pub trait CompareAndApply<I> {
    type Receipt;
    type Authority;
    type Error;

    fn observe(&self, key: &I) -> Result<Self::Authority, Self::Error>;
    fn compare_and_apply(
        &self,
        intent: &I,
    ) -> EffectApply<Self::Receipt, Self::Authority, Self::Error>;
}
```

The orchestration layer should be a generalization or thin composition of
`LatestDurableProjectionCore`, not a replacement that forks its semantics. A
possible shape is `EffectCasCore<K, A, P, R>` with:

- `upsert_desired` / `claim` equivalent to the existing projection core;
- `record_applied`, `record_superseded`, `record_retryable`, and
  `record_rejected` using exact identity + generation + epoch;
- an optional ordered `EffectPlan` whose steps have independent receipts but
  share a lineage;
- a thread-safe reactive shell that exposes pending, in-flight, retry, and
  receipt projections without running I/O while holding the core lock.

Preserve `LatestDurableProjectionCore` as a source-compatible facade or build it
on the new core. Do not force history-preserving consumers onto this API; ordered
fact history remains `TopicCell` / `DurableOutbox`.

## Phases

### 1. Specify the generic protocol in lazily-spec

1. Extend `docs/durable-sinks.md` with a generic effect-CAS section and the
   authority split above. Clarify that "no read/CAS surface reaches the runtime
   effect" means the application callback cannot arbitrate from storage; the
   Lazily-owned adapter may execute an atomic backend comparator.
2. Define state, operations, typed outcomes, invariants, and restart behavior.
3. Add a conformance corpus covering:
   - exact apply and duplicate receipt idempotence;
   - authority mismatch with zero mutation;
   - newer desired intent superseding pending and in-flight retry;
   - stale generation/epoch receipts changing no state;
   - independent keys progressing concurrently;
   - reconnect/crash replay preserving one effect identity;
   - ordered multi-step resume after partial success;
   - supersession between every two plan steps;
   - permanent rejection remaining distinct from retry.
4. Add schema and coverage ledgers without weakening existing
   `LatestDurableProjectionCore` conformance.

Exit: the protocol can be implemented by a pure reference model, and its
cross-backend atomicity limit is explicit.

### 2. Implement the Lazily core and shells

1. Extract/generalize the existing projection algebra into `EffectCasCore`.
2. Add stable intent, attempt, lineage, and receipt types; avoid strings for
   outcome routing.
3. Implement the thread-safe shell with invalidation outside the core lock.
4. Add deterministic logical-clock retry inputs; no worker thread or executor is
   owned by the pure core.
5. Keep `LatestDurableProjectionCore` source-compatible and prove its old
   fixtures replay identically through the new substrate.
6. Add property/model tests for monotone frontiers, at-most-one in-flight attempt
   per key, exact-receipt matching, no mutation on failed CAS, and convergence to
   the latest desired epoch.
7. Publish a Lazily release and pin the exact version/revision for agent-doc.

Exit: Lazily's full checks and conformance suite are green, and an example
in-memory adapter demonstrates the complete claim/apply/receipt cycle.

### 3. Add agent-doc backend adapters

Implement adapters without orchestration policy:

- **Document/editor authority:** compare the canonical editor/controller
  authority hash and apply the editor-native document mutation on its existing
  ordered document lane.
- **Git:** use `git update-ref <ref> <new> <expected>` as the atomic comparator;
  commit-object construction remains preparation, not application.
- **Controller and claim ledger:** compare lineage/version and mutate in one
  SQLite/controller transaction.
- **Snapshot:** write/rename only for the canonical document receipt lineage;
  repeat is idempotent.
- **Worktree/archive cleanup:** key cleanup by the applied document receipt and
  record completion so retries are harmless.
- **Ops evidence:** append only after the effect it describes has a receipt;
  diagnostic reads remain pure.

Each adapter gets contract tests showing that a changed authority returns
`Superseded` and leaves its backend byte-for-byte/logically unchanged.

### 4. Migrate deferred queue maintenance first

1. Represent the maintenance request as one Lazily desired intent keyed by
   document, with the admitted authority hash as its initial observed version.
2. Coalesce new admissions by epoch so only the latest request remains desired.
3. Replace `BackgroundQueueMaintenanceLease`,
   `PendingMaintenanceEffectFence`, and `QueueMaintenanceEffectFamily` policy
   with a Lazily claim and ordered effect plan.
4. Make the canonical document CAS the first applied step. Controller, ledger,
   snapshot, worktree, and ops steps consume that receipt rather than the stale
   admission hash.
5. Persist plan progress/receipts in the existing controller state so a crash
   resumes the same intent identity instead of recapturing work.
6. Delete the bespoke per-family fence checks only after parity tests pass.

Required regression gates:

- authority changes at each of the six current effect-family barriers (`Ops`,
  `Controller`, `Document`, `Ledger`, `Snapshot`, `Worktree`);
- no stale document, controller, ledger, snapshot, worktree, or ops mutation;
- partial-plan recovery performs no already-receipted effect twice;
- a newer admitted request wins while the incumbent is running;
- clean preflight admission remains below five seconds and does not wait for
  comprehensive queue maintenance;
- `session-check` and inspection paths stay read-only.

### 5. Migrate matching agent-doc CAS seams

Inventory every bespoke compare/fence/lease before deletion. Migrate seams whose
semantics are latest projection or receipted effect execution, including:

- Git closeout and parent-ref updates;
- retained editor-write delivery and terminal receipt handling;
- controller document-maintenance and retained-intent lineage fences;
- actor-generation effects where a stale actor receipt must be rejected;
- tmux/layout projection effects that already use latest-wins semantics.

Do not migrate:

- pure in-memory compare-and-swap with no external effect;
- ordered history, which stays on `TopicCell` / `DurableOutbox`;
- backend locks required to implement one adapter's atomic operation;
- domain validation that is not effect ownership;
- transport, process-launch, and read-resilience retries that carry no
  expected authority (nothing is compared-and-applied, so there is no
  supersession for Lazily to own);
- in-process JVM/JS compare-and-set guards in the editor plugins (once-flags,
  single-flight scheduling of local UI work).

For each migrated seam, land parity tests first, switch callers, then remove the
old state machine and assert by source audit that no second retry/supersession
loop remains.

### 6. Rollout and deletion

1. Run Lazily and agent-doc full checks plus deterministic simulation/barrier
   suites.
2. Dogfood behind a temporary feature/config switch that can compare old and new
   decisions without executing both effects.
3. Observe counters for claimed, applied, superseded, retried, rejected, stale
   receipt, and resumed plan steps.
4. Make Lazily CAS effects authoritative after a bounded dogfood window.
5. Delete the compatibility switch, bespoke orchestration traits, duplicate
   retry loops, and obsolete state fields.
6. Update agent-doc architecture/spec comments to make Lazily ownership a
   code-enforced dependency boundary.

## Migration ledger

`tasks/agent-doc/lazily-cas-callsite-ledger.tsv` holds one row per current
CAS/fence/lease/retry/supersession callsite: symbol, authority backend, Lazily
adapter, parity test, status, and deletion proof. `tests/lazily_cas_callsite_ledger.rs`
re-runs the source audit and fails on any unledgered callsite, any stale row, a
`migrated`/`deleted` row without a parity test or deletion proof, or an
`absent:` deletion proof whose pattern reappears. The audit definition (scanned
paths, identifier tokens, call markers) lives in that test's module docs.

Statuses: `migrated` (orchestration runs through `lazily::EffectCasCore`),
`lazily-adapter` (narrow `CompareAndApply` backend or its atomic primitive),
`pending-migration` (matching seam, bespoke orchestration still live),
`excluded` (plan's "Do not migrate" list), `triage` (inventoried with
owning-crate defaults, semantics not yet reviewed), `deleted`. Every row is
reviewed: the audit fails on a `triage` row, so a new callsite must be
classified (the `lazily_adapter` column states the review reason) before it
lands. The audit also covers production editor-plugin sources under
`editors/` (Kotlin/Java/TypeScript definitions by keyword, members only, plus
`compareAndSet`-family calls).

Progress (`#implementlazilyowned`):

- agent-doc depends on crates.io `lazily = "0.63.0"`, which ships
  `EffectCasCore` plus `EffectCasCore::durable_state` /
  `EffectCasCore::hydrate`; the interim `[patch.crates-io]` git pin to
  lazily-rs `e0e3b95` is removed (the full-fidelity `EffectCasDurableState` image the
  spec's "Crash recovery hydrates the retained intent ..." clause requires, with
  optional serde).
- Queue-maintenance document CAS: the hand-rolled refusal/re-observe loop in
  `persist_attached_queue_maintenance_with_retry` now runs through
  `queue_maintenance_effect_cas::persist_document_cut`; a retryable refusal is
  Lazily `Superseded { observed }` and the producer re-desires at the observed
  cut as the next epoch.
- Cross-process claim: `BackgroundQueueMaintenanceLease` is deleted. The
  detached worker's ownership is `agent_doc_queue_io::queue_maintenance_claim`,
  a durable Lazily claim persisted in `state.db`
  (`queue_maintenance_claim:<document>` holds the `EffectCasDurableState`). Every
  transition is load → hydrate → operate → store in one IMMEDIATE transaction.
  A worker reconnects as the newest actor generation (fencing a crashed
  predecessor and requeuing its retained request under the same identity);
  each admitted request is one claimed epoch; a newer admission or a lost
  backend lease is Lazily `Superseded`; completion consumes the exact request row
  and coordination lease in the same transaction that receipts the epoch. The
  flock + lease row (`QueueEditGuard`) remain the backend lock behind the claim.
- Ordered effect plan: `QueueMaintenanceEffectFamily` /
  `fence_queue_maintenance_effect` and the `AdmittedAuthorityLineage` are
  deleted. Each barrier is one step of `QueueMaintenanceEffectPlan`, a Lazily
  receipted sequence keyed by document: a step's expected authority is the
  previous step's receipt (a receipted document cut replaces the admission),
  the applied frontier is the plan cursor, and the first mismatch is a sticky
  Lazily supersession that refuses every later step. The barriers stay where
  the effects are; the plan does not yet reorder the maintenance body so the
  document CAS is literally the first step.
- Git ref adapter: `agent_doc_git_io::ref_cas::GitUpdateRef` is the
  `CompareAndApply` adapter whose only comparator is
  `git update-ref <ref> <new> <expected>` (moved ref = `Superseded`, held
  ref/index lock with the ref unchanged = `Retryable`, else `Rejected`).
  `compare_and_update_ref` runs one Lazily epoch; `drive_ref_update` re-desires
  a producer's commit over the observed ref as the next epoch and re-claims a
  retryable lock from Lazily's retained intent. The stage-and-commit HEAD CAS,
  the sibling HEAD CAS, and the parent submodule pointer (its
  `for attempt in 0..3` loop is deleted) all run through it; the closeout fence
  is the `before_apply` guard after the Lazily claim. Contract tests prove a
  moved ref is `Superseded` and leaves the ref byte-for-byte unchanged.
- Controller terminal effect: `CloseoutOwnerCapability::run_terminal_effect`'s
  `terminal_effect_in_progress` `compare_exchange` flag and the separate
  `terminal_effect_landed` flag are deleted. The terminal effect is a Lazily
  single-flight key (`closeout_terminal_effect::TerminalEffectSingleFlight`);
  the landed receipt is its applied frontier, and a failed or unwinding run is a
  Lazily rejection that frees the key.
- Snapshot / worktree-cleanup / ops-evidence adapters: these effects are no
  longer verify-then-act. `PendingMaintenanceEffectFence::apply_effect_step`
  hands the effect to `QueueMaintenanceEffectPlan::apply_effect`, whose step
  adapter compares the observed canonical authority with the step's expected
  authority inside the Lazily claim and runs the effect only on a match; the
  effect's digest (`snapshot:<content hash>`, `worktree:<reconciled hash>` /
  `worktree:unchanged`, `ops:<message hash>`) is the step receipt. A changed
  authority is `Superseded` and the effect is never invoked; an effect error
  is a Lazily rejection. The snapshot baseline is content-addressed and the
  worktree reconciliation converges, so a retried request repeats them
  harmlessly. Contract tests:
  `queue_maintenance_effect_cas::tests::changed_authority_supersedes_each_effect_adapter_and_leaves_its_backend_unchanged`
  and siblings.
- Controller/ledger SQLite adapter: `agent_doc_sqlite::ledger_cas::TerminalTurnLedgerAppend`
  is the `CompareAndApply` adapter for the terminal closeout append. Its
  authority is `TerminalTurnAuthority` (latest checkpointed cycle, bound actor
  session/pane/generation, live `turn_active` lease); `compare_and_apply`
  re-reads it inside one `BEGIN IMMEDIATE` transaction and inserts the facts
  only on a match, so a moved cycle, rebound actor, or live turn is Lazily
  `Superseded` and writes zero rows. `insert_state_events_if_terminal_turn_fence_in_db`
  is now one Lazily epoch through `ledger_cas::append_if_terminal_turn`
  (`FenceChanged` = superseded). The actor-generation fenced writes
  (`store_actor_record_fenced_tx`, `delete_actor_document_fenced_tx`) are the
  next consumers of the same adapter shape.
- Commit-transaction retries: the five caller-side
  `RetryableHeadMoved`/`RetryableIndexLock` loops in `agent-doc-commit-io`
  are deleted. `agent_doc_git_io::ref_cas::drive_commit_transaction` drives an
  exact-path transaction through a Lazily core keyed by the repository HEAD:
  a moved HEAD is `Superseded` and the still-wanted commit is re-desired over
  the observed HEAD as the next epoch, a held lock is `Retryable` and
  re-claimed from Lazily's retained intent, and an ignored path or fatal
  failure is `Rejected`. `pace_commit_transaction` only logs and waits the
  backoff; it never decides whether to retry.
- Triage review: all 517 remaining `triage` rows were reviewed. 280 are
  `excluded` with a stated reason (74 token false positives such as Markdown
  fence parsers, 60 transport/launch/read retries, 58 lease-row primitives and
  liveness predicates, 44 domain supersessions, 23 diagnostics, 9 authority
  backend comparator primitives, and a few ordered-history, schedule-cadence,
  read-only, test-support, and in-memory rows); 237 are `pending-migration`
  matching seams grouped by family (document CAS retry loops, authority
  observation retries, retained editor writes, tmux/layout/focus latest-wins
  projections, actor-generation effects, dispatch/recycle owner supersession,
  closeout/turn-fenced effects, cross-process owner leases, queue projection
  CAS, Git closeout helpers, idle-watch effect retries).
- Final audit extended to `editors/`: 126 editor-plugin rows inventoried and
  reviewed (72 `excluded`: in-process compare-and-set guards, transport and
  attach retries, notices, editor-native locks; 54 `pending-migration`: the
  editor half of the document authority in `CrdtReplicaManager`,
  `PatchWatcher`, and `TypingTracker`, plus retained surface/layout delivery,
  whose retry/supersession must move behind the Rust core so Lazily owns it).
- The 13 original preflight `pending-migration` rows are done. Authority
  observation retries (`observe_current_text_with_bounded_retry_fenced`, the
  native-save reconciliation, the re-arm/repeat evidence reads, and the
  model-ensure re-registration) run through
  `queue_maintenance_effect_cas::drive_authority_observation`: each
  observation is a Lazily-claimed attempt, a transient status is `Retryable`,
  a settled one is the applied receipt, and a fence error is a rejection; the
  bespoke `for attempt in 1..=attempts` loops are gone (10 rows `migrated`).
  `fence_pending_maintenance_effect` and
  `fence_queue_maintenance_authority_acquisition` are deleted: every barrier
  is a `QueueMaintenanceEffectPlan` step (`Verify`, and the new
  `AuthorityAcquisition` step that keeps its test delay), and
  `QueueMaintenanceAuthoritySuperseded` is replaced by the plan's typed
  `QueueMaintenanceEffectSuperseded::admitted_authority` (3 rows `deleted`
  with `absent:` proofs).
- Family 1, actor-generation fenced writes (`#implementlazilyowned`
  continuation, lzowned5): `ledger_cas::ActorGenerationWrite` is the
  `CompareAndApply` adapter (authority = generation + `ActorRevision`, one
  IMMEDIATE transaction); `store_actor_record_fenced_tx`,
  `store_actor_lifecycle_record_fenced_tx`, and `delete_actor_document_fenced_tx`
  are single Lazily epochs, and a loss is the typed `ActorGenerationSuperseded`.
  Every re-deciding caller (the four controller sweeps and the start_session
  alias close, the three registration binds, start's generation race, the
  route lifecycle write) runs through `ledger_cas::drive_actor_generation_decisions`;
  the four string classifiers collapse into `ledger_cas::actor_generation_superseded`
  over `agent_doc_supervisor::actor_generation::ACTOR_GENERATION_SUPERSEDED_MARKER`.
  20 rows: 14 `migrated`, 4 `deleted`, 2 reclassified `lazily-adapter`
  (`LifecycleFencePayload`, `BindingFence`: wire form / value of the expected
  authority), plus 5 new `lazily-adapter` rows.
- Family 2a, cross-process owner leases (lzowned6):
  `agent_doc_sqlite::owner_lease_claim` is a durable Lazily owner claim (an
  `EffectCasCore` image in `state.db`, transitioned in one IMMEDIATE
  transaction; one grant = one `[Grant, Release]` epoch whose retained
  `Release` step is the ownership). `OwnerLeaseWrite` is its `CompareAndApply`
  adapter over a backend row (`CoordinationLeaseBackend`,
  `BootstrapSlotBackend`). The queue drain owner (`refresh` / `read` /
  `clear_drain_owner_lease`), the predecessor-drain slot
  (`record` / `read_predecessor_drain_lease`), and the orphan-drain dispatch
  slot (`claim_orphan_drain_dispatch`, replacing
  `claim_coordination_lease_if_expired_in_db`) run through it. session-check's
  read-then-clear of a loop drain owner is now `release_drain_owner_if`, one
  epoch that compares the owner. 17 rows: 6 `migrated`, 10 `lazily-adapter`
  (freshness/deadline predicates, grant values, backend scopes, and
  `QUEUE_APPLY_LEASE_SCOPE`, the backend lock scope behind
  `queue_maintenance_claim`), 1 `excluded` (`DocumentTurnAuthorityStreamLease`:
  an in-process RAII subscription refcount, no effect ownership), plus 11 new
  `lazily-adapter` rows.
- Family 2b, closeout/turn-fenced effects (lzowned6): a claimed
  `CloseoutOwnerCapability` carries a Lazily tenure
  (`closeout_owner_tenure::CloseoutOwnerTenure`, one claimed `EffectCasCore`
  epoch). An answered refresh loss records it `Superseded`, `lease_lost()`
  reads that terminal record, and the guard's release receipts it. The shared
  `lease_lost` `AtomicBool` is deleted. write-runtime's
  `claim_closeout_owner_until_answered` resend is one Lazily epoch: each send
  is a claimed attempt, a transport error is `Retryable` and re-claimed after
  the unchanged backoff, the cap stays at three, and the
  `closeout_owner_claim_retry` marker is unchanged. The turn-fenced
  frontmatter marks (`PreflightTurnFence`, `TerminalTurnFence`,
  `*_if_turn_fence`, `*_with_turn_fence`, `requires_turn_fence`) already carry
  the expected `TerminalTurnAuthority` into the Lazily terminal-turn append,
  so they are `lazily-adapter`. So are the state-backbone/controller lease TTLs
  (the expiry of the controller-decided claim) and the write-runtime capability
  wrappers. 37 rows: 3 `migrated` (`lease_lost`, `mark_lease_lost`,
  `CLOSEOUT_OWNER_CLAIM_ATTEMPTS`), 33 `lazily-adapter`, 1 `excluded`
  (`cross_session_decision_with_lease`: a pure claim gate over a lease
  freshness bool). There are also 2 new `lazily-adapter` rows. The heartbeat
  thread's transport-retry cadence is unchanged.
- Family 3a, document CAS refusal/re-observe loops and queue projection CAS
  (lzowned7): the new leaf crate `agent-doc-effect-cas` holds the one shared
  driver, `drive_effect_epochs` (an `EffectCasCore` drive keyed by an effect
  identity). Each claimed attempt reports `Applied`, `Superseded { observed }`
  (re-desired over the observed authority as the next epoch), or `Retryable`
  (the same epoch re-claimed); an attempt error is a Lazily rejection. A
  `between` hook runs only before another Lazily claim. It holds the
  diagnostics and the physical wait, and `Pace::Stop` fails closed. The
  following loops now run through it: compact's converge
  (`converge_compact_targets_with_retry`) and commit observation; route-io's
  dispatch enqueue, queue activation, and witness-gated resolve; queue-io's
  claim runtime projection and queue-authority projection-lag re-ask; and the
  `dedupe` fixpoint. Messages, log markers, and attempt caps are unchanged.
  The relay/controller CAS comparators, the claim-ledger generation
  comparators, the consume CAS under `QueueEditGuard`, the `retry_crdt_merge`
  classifiers, and the `settle_before_projection_retry` pace hooks are
  `lazily-adapter`. `fence_compacted_crdt_epoch` (one relay epoch-rotation
  request, no retry) is `excluded`. 27 rows: 11 `migrated`, 15
  `lazily-adapter`, 1 `excluded`, plus 1 new `lazily-adapter` row
  (`RuntimeProjectionRetry`). The 17 document-realtime / idle-watch rows of
  family 3 remain for 3b.
- Family 3b, document-realtime write convergence (lzowned7): the canonical
  replace (`apply_canonical_replace_if_attached`) now runs every pass as one
  `drive_effect_epochs` attempt. A landed, deferred, or retained write is the
  applied receipt, a refusal is a rejection, a lost compare-and-swap
  (`CompareAndSwapRaced`) or operator advance is `Superseded` over the
  observed relay cut, and every other wait state is `Retryable`. The
  convergence `DeadlineCore` stays the bound. The projection-settle
  observations also run through the driver. They are the visible-receipt wait,
  the serialized-atomic-write projection wait, the native-save wait, the
  atomic-repair settle, the retained-write settle, and the visible-write
  current-transition guard. A pending observation is `Retryable`, and a
  deadline is `Pace::Stop` or a terminal receipt. `reconcile_visible_write`
  (document-realtime) is a Lazily drive whose disk drift is `Superseded` and
  re-merged. Messages, log markers, timeouts, and pace curves are unchanged.
  15 rows: 7 `migrated` (the `CRDT_WRITE_BACKOFF_*` /
  `CRDT_PROJECTION_FALLBACK_BACKOFF_*` pace constants,
  `VISIBLE_WRITE_RECONCILE_MAX_ATTEMPTS`), 3 `lazily-adapter`
  (`CrdtWriteBackoff`, `ForceDiskMutationFence`, `force_disk_mutation_fence`),
  and 5 `excluded`. The excluded rows are the relay `RecoveryRetryBackoff`
  re-send latch and its two constants (a transport resend with no expected
  authority) and `CrdtRetryAdmission` / `decide_crdt_retry_admission` (a pure
  decision used only by the simulation model). Idle-watch's
  `QueueProjectionSupersededRetry` / `queue_projection_superseded_retry` are
  scheduler state of the self-heal worker in the same supervisor loop as the
  idle-watch retries. They move to step 8.
- Family 4, retained editor writes and retained-intent lineage (lzowned8):
  the shared crate gains `agent_doc_effect_cas::latest_intent::LatestIntentSchedule`,
  a keyed Lazily latest-intent schedule over `EffectCasCore` (single flight,
  latest-wins supersession by a newer desire, Lazily-owned retry due time and
  attempt count, exact receipt matching). A refused native save
  (`schedule_refused_retained_persistence_retry`) is now the document's newest
  retained intent whose expected authority is the refused delivery epoch; the
  timer thread only wakes the claim, and `claim_retained_persistence_retry`
  compares that epoch with the latest desired save, so a newer epoch or a held
  single flight is Lazily `Superseded` (the bespoke `desired.epoch != epoch`
  check is gone). The retained editor-op drainer's `in_flight` set and the
  head burst's `attempts` / `next_attempt_at` are deleted: the head burst of
  each document's FIFO is the Lazily intent (expected = its base hash, payload
  = its nonce) holding the document's single flight, and a transient append is
  `retry_after` with the unchanged backoff; the FIFO stays the typing-order
  history. `consume_replayed_editor_op_epoch` runs its three clears through
  `drive_effect_epochs` (a failed clear over a projected epoch is `Retryable`,
  an absent epoch is the applied receipt). Messages, log markers
  (`retained_persistence_retry`, `editor_ops_recorded ... attempts=`), retry
  caps, and pace curves are unchanged. 23 rows: 6 `migrated`, 16
  `lazily-adapter` (pace/escalation inputs, one-shot compare-and-retire
  primitives, per-document atomicity locks, the retained-for-retry marker and
  its classifier, the one-shot retained-response saves, per-attempt response
  budgets), 1 `excluded` (`live_exchange_without_visible_write_retry_required`:
  a pure predicate with no production caller), plus 5 new rows (4 `migrated`:
  `claim_retained_persistence_retry` and the schedule's `IntentRetry` /
  `retry_after` / `superseded`; 1 `lazily-adapter`:
  `RETAINED_PERSISTENCE_RETRY_EFFECT`).
- Family 5, tmux/layout/editor-focus latest-wins projections (lzowned8):
  `LatestTmuxEffectWorkerState` is backed by a Lazily `EffectCasCore`
  latest-wins key: every `schedule` desires its revision as the key's epoch
  (a stale revision is Lazily `StaleEpoch`), and `is_superseded` /
  `pending_revision` read the Lazily latest epoch; `active` only guards the
  worker thread's spawn. `TmuxEffectAttemptLedger`'s `attempts` counter is
  deleted: a generation is one `LatestIntentSchedule` intent, each admitted
  attempt is a Lazily claim, the previous unconverged attempt is recorded
  `Retryable` at the next admission, a claim past
  `TMUX_EFFECT_RETRY_ATTEMPT_LIMIT` is a Lazily rejection, and a retirement
  records the claimed attempt rejected (budget) or superseded (yield, forget,
  newer generation). The `pane_layout_desired` projector's bespoke `attempt`
  counter is deleted: each lane's pending frame is a Lazily latest-wins intent
  (expected = plane version), the worker claims it, a failed projection is
  `retry_after` with the unchanged backoff, and a newer frame supersedes a
  retrying one. `EditorSurfaceRetry` is a Lazily retained intent per
  (project root, editor client) whose expected authority is the surface it
  failed under: a moved surface is `Superseded`, an accepted effect is its
  receipt. `LatestIntentSchedule::claim` now rounds `now` up to the Lazily
  millisecond clock so a zero-delay retry is claimable at once. Messages, log
  markers, the twelve-attempt cap, and the 250ms..5s pace are unchanged.
  31 rows: 9 `migrated`, 9 `lazily-adapter` (the focus-fence comparators over
  the Lazily `ThreadSafeSourceMap` latest focus intent, the generation
  worker's pace wait, the budget accessor), 13 `excluded` (the route-lease
  RAII publication holds, the four "superseded plugin bytes" route-admission
  verdicts, and the best-effort superseded tmux session cleanups), plus 3 new
  rows (`EditorSurfaceRetryStore` `migrated`; `EDITOR_SURFACE_RETRY_EFFECT`,
  `editor_surface_retry_authority` `lazily-adapter`).
- Dispatch/recycle retry budget and owner supersession (plan step 6): the
  recoverable-dispatch retry and the stale-binary / stale-generation redirect
  inside `authorize_dispatch` are two Lazily drives (`drive_effect_epochs`): a
  stale-generation redirect is `Superseded { observed: <generation> }`, so the
  retry targets the observed generation as the next epoch; a stale binary and
  a recoverable refusal are `Retryable`. The self-recycle handoff's
  `recycle_handoff_in_flight` `compare_exchange` flag is deleted: the handoff is
  a Lazily single-flight key (`agent_doc_effect_cas::single_flight::SingleFlight`,
  shared by future single-flight effects) whose flight moves into the handoff
  thread, a promotion or supersession receipts it, and every other outcome
  releases it. `superseded_owner_replacement_step` and its `requested_at_ms`
  timer are deleted: `SupersededOwnerReplacement` is a Lazily retained intent
  (request = desire + claim, recorded `Retryable` due `wait_secs` later; a due
  claim is a sticky rejection), used by the dispatch-only recycle gate and the
  SimWorld model. The busy-composer steering submit (`ROUTE_SYNC_SUBMIT_ATTEMPTS`)
  and the context-clear resubmit loop (its `attempts_sent` counter) run through
  `drive_effect_epochs`. `RegistryPublicationRetryState` is a Lazily
  `EffectCasCore` retained intent with the idle tick as logical time (its
  `PublicationReceipt` attempt/next-tick state is deleted). Messages, log
  markers, the one-retry caps, the 750ms backoff, the 45s replacement wait, the
  three submit attempts, and the 1..32 tick backoff are unchanged. 45 rows:
  9 `migrated`, 11 `lazily-adapter`, 2 `deleted`, 23 `excluded` after review
  (the readiness-poll `RetryBudget` family, the queue-claim continue/retry
  lifecycle classifiers, the busy-pane remediation ladder, the duplicate-owner
  process retirement and route-owned superseded-binding reap, a diagnostic
  inject counter, and a one-shot projection compare-and-remove), plus 5 new
  rows (`SupersededOwnerReplacement` `migrated`; 4 `lazily-adapter` identities
  and attempt budgets). The renumbered `compare_exchange` occurrence keeps its
  excluded review; the deleted handoff flag is
  `compare_exchange(recycle_handoff_in_flight)`.
- Editor document-authority retries, first half (plan step 7a): the
  JetBrains plugin's retry refusal counts move behind the FFI onto a Lazily
  failure ledger (`agent_doc_effect_cas::failure_ledger::FailureLedger`, a
  `LatestIntentSchedule` per scope whose attempt number is the count; exported
  as `agent_doc_effect_failure_{record,count,clear,clear_scope}`). The Kotlin
  `EffectFailureLedger` binds it per family and owner scope
  (`crdt.local-editor-flush`, `crdt.replica-registration`,
  `crdt.template-guard-recovery`, `crdt.deferred-write-replay`,
  `crdt.undurable-local-forward`, `typing.base-hash`), reads the last Lazily
  count on the EDT (where native calls are forbidden), owes EDT records and
  clears to the native ledger in order, and falls back to the previous
  plugin-side count when the loaded library predates the ABI (learned once per
  native generation; never throws). The managers keep only pace curves (the
  three inline doubling curves are one `crdtDoublingBackoffMsUtil`) and timers.
  The write-only `registerRetryAfterMs` / `registerFailureCounts` maps and the
  uncalled `PatchWatcher.scheduleRepositionRetry` are deleted. Messages, log
  markers, the 5 base-hash and 8 settle bounds, and every pace are unchanged
  (the local-flush count is no longer clamped at 13; its delay exponent was
  already clamped at 12). 41 rows: 19 `migrated`, 6 `deleted`, 2
  `lazily-adapter` (the patch-generation comparator over the binary-minted
  baseline hashes, which runs under the EDT capture and refuses back to the
  binary), 14 `excluded` (the read-lock capture-cut retry, the single-flight
  timer sets and timers, the no-op drain poll pace and gate, the
  non-operator-epoch capture fences, the generation-fence normalizers), plus
  1 new row (`crdtDoublingBackoffMsUtil` `migrated`).
- Editor retained surface/layout delivery, second half (plan step 7b): the
  classic/backend `EditorTabSyncListener` retained-surface delivery refusal
  count is the Lazily failure ledger's (`EffectFailureLedger`
  `surface.delivery`; `SurfaceDeliveryRetry` stays its pace curve, cleared on
  an accepted delivery, a new observation, and shutdown). The uncalled
  `SyncLayoutAction` deferred-sync helpers (`SYNC_DEFERRED_RETRY_MS`,
  `SYNC_DEFERRED_MAX_RETRIES`, `deferredSyncRetryDelayMs`,
  `shouldRerunAfterSupersede`) and their tests are deleted: the controller
  owns layout sync through Lazily latest-wins keys (step 5). The 262 Remote
  Dev frontend is shell-only (no native library, GH #339), so no Rust core can
  own its rows: the ingress connection lease is a carrier of the
  backend-owned lease the backend compares (`STALE_LEASE`), its reconnect pace
  is transport resilience, and the bounded presentation recapture is a read
  the backend asks for (`retrySuggested`) and re-derives from. Messages, log
  markers, and paces are unchanged. 13 rows: 2 `migrated`, 5 `deleted`, 2
  `lazily-adapter`, 4 `excluded`.
- Authority observations and Git commit/closeout helpers (plan step 8a):
  the operator re-arm evidence read (`operator_rearm_evidence_for_merge`) and
  the missing-replica re-registration self-heal
  (`reobserve_missing_editor_replica_with_reregistration`) run through
  `drive_effect_epochs`: an unreadable read and a still-missing replica are
  `Retryable`, a served replica is the receipt, and an endpoint every live
  route refused stops the drive (`Pace::Stop`). The commit path's two
  deadline-bounded waits, the owned-delivery rebase
  (`retry_rebase_until_owned_delivery`) and the controller commit ask
  (`retry_controller_commit_awaiting_projection`), are Lazily drives too
  (an undelivered owned target or a projection-awaiting refusal is
  `Retryable`; the wait and re-read run between attempts; other refusals and
  the exhausted budget are returned unchanged). The commit transaction fence,
  the sibling fence carrier, the post-commit snapshot/HEAD witness, and the
  commit backoff curve are `lazily-adapter`s of the existing Lazily
  commit-transaction / `ref_cas` drives. Messages, log markers, attempt caps,
  budgets, and paces are unchanged. 14 rows: 9 `migrated`, 5
  `lazily-adapter`.
- Idle-watch captured-finalize resume and queue projection self-heal (plan
  step 8b): both retained intents of the idle-watch supervisor are a
  `LatestIntentSchedule` (`agent-doc-start-runtime-io`
  `idle_watch_schedules.rs`). `CapturedFinalizeResumeSchedule` replaces
  `CapturedFinalizeResumeRetry`'s `attempts` / `retry_at`: the attempt number
  logged and paced is Lazily's, the due time is the retained intent's ready
  time, and each per-outcome attempt reset is a fresh Lazily epoch (a
  live-owner lease records the attempt `Superseded` and retains a fresh epoch
  retimed to the lease deadline, so the next attempt is 1; an operator or
  unlandable verdict counts as the fresh epoch's first refusal, so the next is
  2; an unlandable capture is a Lazily rejection; a state wait, a terminal
  receipt, or an absent operation retires the epoch). A document edge retimes
  the retained intent to now (the pre-migration `retry_at = now`) and owns the
  next attempt. `QueueProjectionSelfHealSchedule` replaces the
  `queue_projection_self_heal_due` / `_retry_at` pair and
  `QueueProjectionSupersededRetry`: the claimed attempt rides the worker,
  refusals are `defer`red (`retry_after`, or a retime of a repair requested
  mid-attempt), and a superseded attempt is recorded `Superseded { observed:
  <watermark> }` with one new repair due after the quiet window; the quiet
  window stays the caller's operator-typing debounce. `LatestIntentSchedule`
  gains `is_due`, `retime` (through Lazily's durable image, the crash-recovery
  path), and `withdraw`. Messages, log markers, and paces are unchanged. Race
  difference: a retryable outcome of an attempt whose operation key changed
  mid-flight no longer leaves a timed retry for the old key (Lazily records it
  `Superseded`; the new key's fresh epoch owns the next attempt). 11 rows: 8
  `migrated` (the 5 delay/edge rows plus 3 new schedule methods), 3
  `deleted`.
- Plan step 9b (per-step plan receipts for crash resume): the
  `QueueMaintenanceEffectPlan` keeps a durable image
  (`QueueMaintenancePlanProgress`: the admission, the Lazily applied
  frontier, and every effect receipt and receipted document cut; verify-only
  receipts change no state and are not retained). `BackgroundRequestEffectFence`
  stores it in `state.db` beside the claim (`queue_maintenance_plan:<document>`)
  after every step that applies an effect or receipts a cut, through
  `QueueMaintenanceClaim::store_plan_progress`, whose IMMEDIATE transaction
  first proves the writer still holds that exact request in flight (a fenced
  predecessor cannot overwrite a successor). A worker that resumes the same
  request reloads it (`QueueMaintenanceEffectPlan::resume`, ops line
  `queue_maintenance_plan_resumed`): the plan continues the same identity
  sequence and expects the last receipted cut, so a crash after the document
  cut is resumed instead of being rejected as a superseded admission. Replayable
  effects (ops evidence lines and content-addressed snapshot checkpoints, the
  effects a repeat could only duplicate) cross `apply_replayable_effect`: a
  `(step, intent)` that already has a resumed receipt is receipted under the
  same authority comparison without running again; convergent reconciliations
  (worktree inventory, claim/tombstone/completion reconciles, the queue-head
  snapshot adoption) re-run, harmlessly. Progress of any other request version
  is never resumed, and `QueueMaintenanceClaim::complete` deletes it in the
  transaction that consumes the request. Tests:
  `queue_maintenance_effect_cas::tests::resumed_plan_continues_from_the_receipted_cut_without_repeating_effects`,
  `resumed_replay_still_supersedes_on_authority_drift`,
  `progress_of_another_admission_is_ignored`,
  `queue_maintenance_claim::tests::plan_progress_resumes_only_the_exact_claimed_request`,
  `preflight_command_io::tests::successor_resumes_a_dead_workers_plan_from_its_receipted_cut`.
  No ledger rows change (no new CAS/fence/lease/retry identifiers). A crash
  between the editor CAS write and its cut receipt still resumes from the
  previous receipt and is rejected as superseded, as before.
- Plan step 9a (reorder so the document cut is the first plan step) is not
  landed: see "Resume here" step 9 for the read-after-write dependencies that
  block a behaviour-preserving reorder.
- Not yet done (as of 2026-11-15, agent-doc `59984d315a`; triage is complete,
  0 `triage` rows): migrate the 291 reviewed `pending-migration` rows family
  by family, then delete the remaining bespoke orchestration and make the
  final audit show no second application-owned retry/supersession loop.

## Resume here

Status 2026-11-15 (after step 9b): ledger = 133 `migrated`, 173 `lazily-adapter`,
33 `deleted`, 419 `excluded`, 0 `pending-migration`, 0 `triage`. Every
inventoried row is migrated; work the remaining steps sequentially in ONE
worktree, one step per pushed commit, `make check` green before each push, in
this order (next: step 9a, then step 10):

1. DONE: actor-generation fenced writes (`store_actor_record_fenced_tx`,
   `delete_actor_document_fenced_tx`; 20 actor-generation rows) onto the
   `ledger_cas` adapter shape (`ActorGenerationWrite`,
   `drive_actor_generation_decisions`).
2. DONE: closeout lease and drain-owner claims onto a durable Lazily claim, as
   `queue_maintenance_claim` does, in two pushed halves:
   - 2a. DONE: the 17 cross-process owner-lease rows (queue drain owner,
     predecessor-drain slot, orphan-drain dispatch slot) onto
     `agent_doc_sqlite::owner_lease_claim`.
   - 2b. DONE: the 37 closeout/turn-fenced rows (`CloseoutOwnerCapability` lease
     loss, the closeout-owner claim retry, captured-finalize resume lease,
     the `*_if_turn_fence` / `*_with_turn_fence` frontmatter marks, and the
     state-backbone lease constants) onto a Lazily claim + the ControllerLedger
     SQLite adapter.
3. Document CAS refusal/re-observe retry loops (29 rows) plus queue
   projection CAS (15 rows), in two pushed halves:
   - 3a. DONE: compact-io, route-io `queue_dispatch`, queue-io `queue_cmd` /
     `queue_consume`, the dedupe fixpoint, and the comparator/classifier/pace
     rows, onto `agent_doc_effect_cas::drive_effect_epochs` (27 rows + 1 new).
   - 3b. DONE (15 rows; the 2 idle-watch self-heal rows moved to step 8):
     the document-realtime write-convergence
     backoffs (`CrdtWriteBackoff`, the `CRDT_WRITE_BACKOFF_*` /
     `CRDT_PROJECTION_FALLBACK_BACKOFF_*` policies inside
     `apply_canonical_replace_if_attached` and the projection-settle polls),
     the relay `RecoveryRetryBackoff` re-send latch, `CrdtRetryAdmission`,
     `ForceDiskMutationFence`, `VISIBLE_WRITE_RECONCILE_MAX_ATTEMPTS`
     (`reconcile_visible_write` in write-runtime-io), and idle-watch's
     `QueueProjectionSupersededRetry`.
4. DONE: retained editor writes and retained-intent lineage (23 rows + 5 new)
   onto the Lazily latest-intent core
   (`agent_doc_effect_cas::latest_intent::LatestIntentSchedule`).
5. DONE: tmux/layout/editor-focus latest-wins projections (31 rows + 3 new)
   onto Lazily latest-wins keys (`LatestTmuxEffectWorkerState`,
   `TmuxEffectAttemptLedger`, the `pane_layout_desired` projector,
   `EditorSurfaceRetryStore`).
6. DONE: dispatch/recycle retry budget and owner supersession (44 + 1 rows
   + 5 new) onto `drive_effect_epochs`, a Lazily retained intent
   (`SupersededOwnerReplacement`, `RegistryPublicationRetryState`), and the
   shared Lazily single flight (`agent_doc_effect_cas::single_flight`).
7. The editor half of the document authority (`CrdtReplicaManager`,
   `PatchWatcher`, `TypingTracker`; 41 + 13 editor rows): move
   retry/supersession behind the Rust core (FFI) so Lazily owns it.
   - 7a. DONE: the 41 `CrdtReplicaManager` / `PatchWatcher` / `TypingTracker`
     rows onto the FFI Lazily failure ledger (`EffectFailureLedger` over
     `agent_doc_effect_cas::failure_ledger`).
   - 7b. DONE: the 13 retained surface/layout delivery rows
     (`EditorTabSyncListener` `SurfaceDeliveryRetry`, `SyncLayoutAction`
     deferred sync, the 262 frontend `SurfaceIngressLease`).
8. Remaining small families:
   - 8a. DONE: authority observation (6) and Git commit/closeout helpers
     around `ref_cas` (8) onto `drive_effect_epochs` / `lazily-adapter`.
   - 8b. DONE: idle-watch/captured finalize retry (6, plus idle-watch's
     `QueueProjectionSupersededRetry` / `queue_projection_superseded_retry`
     self-heal schedule from family 3) onto `LatestIntentSchedule`
     (`CapturedFinalizeResumeSchedule`, `QueueProjectionSelfHealSchedule`),
     per-outcome attempt resets as fresh Lazily epochs.
9. Per-step plan receipts persisted for crash resume, and reorder the
   maintenance body so the document cut is literally the first plan step.
   - 9b. DONE: per-step plan receipts in `state.db` beside the claim; a
     resumed request continues from the last receipted cut and skips every
     replayable effect that already has a receipt (see Progress).
   - 9a. OPEN: the reorder. In `run_queue_maintenance_with_options_under_apply_guard`
     (agent-doc-preflight-io) the pre-cut effects are not all deferrable,
     because later pure computation of the successor reads back what they
     wrote: `queue_claim::transfer_promoted_claims`,
     `transfer_prompt_identity_admissions`, `release_closed_head`, and
     `reconcile_completion_evidence` are read back by
     `load_ledger_following` / `owner_selection_claims_for_content` before
     the lifecycle markers are painted into the document;
     `queue_tombstone::reconcile_for_file` returns the sets the backlog mirror
     uses; `adopt_edited_queue_head_into_snapshot` is re-read through
     `load_document_baseline` for the snapshot-drift decisions; the worktree
     reconcile returns content the cut persists; and the Controller
     observation publishes the admitted authority the projections read. The
     write-only pre-cut effects (the ops evidence lines,
     `set_skipped_queue_head_ids`, `set_projected_in_progress_queue_heads`,
     `record_mirrored_queue_heads_in_db`) can move behind the cut as they
     are. A literal first-step cut needs those reads to come from an
     in-memory overlay of the pending ledger/tombstone/snapshot writes (pure
     `*_plan` variants in agent-doc-queue-io), so the successor is computed
     without writing and the effect phase then applies the overlay after the
     cut receipt. The halt, time-gate, and main exits each persist a cut and
     must flush the effect phase after it.
10. Phase 6 rollout: delete compatibility paths and duplicate state, final
    source audit, then `make install` + recycle (coordinator only).

Per family: parity/`Superseded` contract test first, switch callers, delete
the old loop, mark rows `migrated`/`deleted` with `absent:` proofs, update this
Progress list, push. Preserve messages, log markers, and retry caps unless the
ledger row states why they change. Use a paired worktree
(`<parent>/<label>/agent-doc` + private `<parent>/<label>/tmux-router` detached
at `2a9dfdd`; the superproject pointer `f4acd6d` lacks `RegistryLockWait`).
Tracking item: `#implementlazilyowned` in agent-loop `tasks/software/lazily.md`.

## Integration order

1. lazily-spec protocol + fixtures
2. lazily-rs core, shell, compatibility facade, and release
3. agent-doc dependency update and adapters
4. queue-maintenance migration
5. remaining matching CAS seams
6. dogfood, delete compatibility path, release agent-doc

Keep these as separately reviewable commits/PRs. The current bounded-preflight
correctness fix should land first; this migration then replaces its bespoke
fence machinery under already-green behavioral regressions.

## Completion criteria

- Lazily is the only owner of effect identity, single-flight, supersession,
  retry/coalescing policy, and receipt matching.
- agent-doc adapters contain atomic backend operations but no effect scheduler or
  latest-wins state machine.
- every stale or failed comparison is typed and produces no backend mutation.
- queue maintenance resumes after crashes and authority changes without stale or
  duplicate effects.
- preflight stays under five seconds because comprehensive maintenance remains a
  coalesced background effect.
- source audits find no remaining agent-doc copies of the migrated orchestration
  protocol.
