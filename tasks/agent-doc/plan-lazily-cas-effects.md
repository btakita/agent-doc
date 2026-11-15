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
- Not yet done: the 13 original preflight `pending-migration` rows (authority
  observation retry `observe_current_text_with_bounded_retry_fenced` and
  friends, `fence_pending_maintenance_effect`, `run_pending_maintenance_fenced`,
  `fence_queue_maintenance_authority_acquisition`,
  `QueueMaintenanceAuthoritySuperseded`), then the reviewed
  `pending-migration` families above, moving the remaining `controller-ledger-sqlite` seams (actor generation,
  closeout lease, drain-owner claims) onto the SQLite adapter, persisting
  per-step plan receipts for crash resume inside one request (today a resumed
  request re-derives its idempotent effects), reordering the maintenance body
  so the document cut is the first plan step, triage of the `triage` rows, and
  deletion of the remaining bespoke orchestration.

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
