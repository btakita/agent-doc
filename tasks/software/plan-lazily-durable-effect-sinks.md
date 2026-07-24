# Plan: Durable writes as Lazily effect sinks

## Decision

Encourage one direction of authority:

```text
cold durable state ──hydrate once──▶ Lazily live state
                                      │
                                      ▼
                         computed projection or event stream
                                      │
                                      ▼
                             Effect / AsyncEffect
                                      │
                                      ▼
                            write-only durable sink
                                      │
                               ack / failure
                                      └────────▶ Lazily live state
```

Lazily should make this architecture easy to discover and hard to accidentally
invert. It should not become a database framework. Applications continue to own
serialization, transactions, schema migration, storage selection, and startup
recovery.

The invariant is:

> While a Lazily runtime is live, transitions are decided from Lazily state.
> Durable storage receives projections or ordered facts as an effect. A sink
> must not reload storage and use it to arbitrate the transition it is
> currently persisting.

This makes durable storage an effect sink without claiming that all effects are
durable or that durable storage is always eventually consistent.

## Preserve the projection/history distinction

Ordinary `Effect` and `AsyncEffect` observe settled reactive state. Lazily
already coalesces effect reruns across a batch, so a batch containing
`A → B → C` may persist only `C`. That is correct for a durable projection such
as current actor state.

Durable history has a different contract: every accepted fact must survive and
remain ordered. It should use the existing `TopicCell`/`DurableOutbox` family,
with a stable cursor, replay, idempotency key, and monotone acknowledgement.
Do not modify ordinary effects to retain intermediate values; that would weaken
their existing coalescing semantics and duplicate the ordered-stream
primitives.

Before choosing an API, an agent must answer:

1. Is this sink persisting the latest projection or an ordered history?
2. Must the transition be durable before it becomes externally visible?

Those two answers determine the shape:

| Need | Lazily source | Sink contract |
| --- | --- | --- |
| Latest recoverable state | `Computed` read by `Effect`/`AsyncEffect` | Idempotent upsert of the latest epoch |
| Every accepted fact | `TopicCell` or existing `DurableOutbox` | Append/replay/ack with a stable cursor |
| Durable before visible | Ordered fact plus application acknowledgement cell | Visibility waits for monotone `durable_through` |
| Ephemeral state | Presence/ephemeral primitives | Persistence rejected |

## Phase 1: guidance before API

Add one short “durable sinks” section to the Lazily README, spec, and agent
harness instructions. It should state:

- Live coordination, compare-and-swap, ownership, deduplication, leases, and
  transition selection happen in Lazily state.
- Durable I/O runs from `Effect`/`AsyncEffect`, or from a
  `TopicCell`/`DurableOutbox` drain when history must be lossless.
- A runtime sink is write-only with respect to transition authority. Loading and
  migration belong to a separate startup hydrator.
- Computed values and transition reducers stay pure.
- Success advances a monotone acknowledgement such as `durable_through`.
- Failure remains represented in live state as pending, retrying, or
  backpressured. It does not trigger a database reload at the decision seam.
- Values on the existing `Ephemeral` plane must not enter a durable sink; reuse
  the existing `Durable` marker rather than inventing another marker hierarchy.

This phase should include two examples only:

- a coalesced current-state projection; and
- a lossless ordered fact sink.

The examples should show an application-owned store trait. Lazily must not ship
SQLite, filesystem, or cloud-store adapters as part of this decision.

## Phase 2: dogfood an application-local adapter

Use agent-doc as the first proving ground, but keep the adapter in agent-doc
until a second consumer demonstrates a shared abstraction.

For closeout state:

1. Hydrate the actor once from durable state at controller startup.
2. Decide claims, heartbeats, phase changes, deduplication, and supersession
   through the live actor.
3. Emit a projection epoch or ordered state fact.
4. Drain it through an effect into the existing transaction store.
5. Feed success or failure back to the actor as
   `durable_through(epoch)` or a retryable failure.
6. Let callers choose an explicit policy:
   - `eventual_projection`: live state is immediately authoritative;
   - `durable_before_applied`: external visibility waits for the acknowledgement;
   - `ephemeral`: no durable sink.

Use existing causal receipts, outbox/cursor concepts, and durability markers
where they already fit. Do not generalize `durable_before_applied` into a new
cross-runtime transaction system during this phase.

The dogfood is successful when agent-doc removes hot-path SQLite reads and
`flock` arbitration from the migrated transition while retaining restart
recovery and exact-once observable closeout.

## Phase 3: cheap enforcement

Prefer local type and module boundaries over a large framework:

- Give the effect a narrow application-owned trait with write operations only.
- Keep hydration/query interfaces in a different module and do not pass them to
  the runtime effect.
- Add architecture tests that fail if hot-path actor modules import the
  persistence query/CAS or file-lock modules.
- Add a compile-fail example showing that `Ephemeral` values cannot satisfy a
  durable sink bound.
- Add one property test per policy:
  - batched projection persists only the settled epoch;
  - ordered history replays every unacknowledged epoch;
  - acknowledgements are monotone and duplicate delivery is idempotent;
  - a sink failure never rolls live authority backward by rehydrating storage.

The agent harness should flag a design that combines a hot-path transition with
`SELECT`, storage CAS, or `flock`, and ask whether the operation is really cold
hydration or actorless bootstrap. This is an architecture-review prompt, not a
generic AST linter or a runtime daemon.

**Status — largely present where tractable; one item blocked.**
- Compile-fail `Ephemeral`↔`Durable`: DONE in lazily — `presence.rs:16`
  (`compile_fail` doctest: a durable sink generic over `Durable` rejects an
  `EphemeralValue`).
- Property laws: DONE in lazily — `tests/relay_spill.rs` covers ordered-history
  replay every unacked epoch (`spill_lossless`) + monotone/idempotent ack
  (`spill_replay_idempotent`); `tests/relay_core.rs` covers projection batch-
  coalesce to the settled value.
- Architecture-review prompt: DONE — the agent-doc `AGENTS.md` carries the
  `#lzdurablesink` review prompt.
- Architecture TEST (fail if hot-path actor modules import persistence query/CAS/
  flock): **BLOCKED** on Phase 2 hot-path cleanliness — `mark_committed` is still
  local and the `mark_*` actorless fallbacks still touch `state.db`, so the
  boundary isn't clean enough to enforce yet. Add this once `mark_committed` and
  queue-consume (todo 5) are fully on the authority.

## Phase 4: promote only proven common structure

After agent-doc and one unrelated application use the same shape, review the
duplicated code. Promote a Lazily API only if all of these are true:

- it is storage-independent;
- it works for both sync and async effects;
- it composes with the existing `DurableOutbox`, `TopicCell`, `Durable`, and
  causal receipt concepts;
- it adds no new scheduler, graph node, persistence backend, observer system,
  or transaction coordinator;
- it replaces more application code than it adds to Lazily; and
- its contract can be expressed with a small conformance fixture and a few
  laws.

A plausible promoted helper is a small policy/acknowledgement vocabulary around
an existing effect. A generic ORM, repository layer, “durable cell” graph node,
or built-in SQLite sink is out of scope.

### Phase 4 promotion review — DEFER (criterion 5 not yet met)

Now that two consumers share the shape (agent-doc closeout + community-pulse
`set_budget`), review the duplicated code against the six criteria:

| # | Criterion | Verdict |
|---|---|---|
| 1 | storage-independent | ✅ `CommandSubmit`/`CausalReceipt` are storage-agnostic |
| 2 | sync + async effects | ⚠️ sync only exercised so far |
| 3 | composes with `DurableOutbox`/`TopicCell`/`Durable`/receipts | ✅ uses `CausalReceipt` |
| 4 | no new scheduler/graph node/backend/observer/txn-coordinator | ✅ just helpers |
| 5 | **replaces more app code than it adds to lazily** | ❌ **not yet** |
| 6 | small conformance fixture + laws | ✅ |

The genuinely shared, domain-independent surface is small: ~10 lines each for
(a) `CommandSubmit` construction from a typed payload + `(namespace, name,
payload_type, policy)` + payload hashing, and (b) `Result<(), String> →
CausalReceipt` mapping. The two consumers don't even share an identical
`payload_hash` (agent-doc uses SHA-256; community-pulse uses `DefaultHasher`),
so extraction would *add* standardization work. With two consumers that is
roughly break-even, not a net reduction — criterion 5 fails.

**Decision: do NOT promote yet.** Re-run this review after a third consumer
adopts the shape (or after the shared surface grows), at which point a small
`lazily` helper — `CommandSubmit::domain_command(...)` + a receipt-from-outcome
fn — would clearly pass criterion 5. This matches the plan's "promote only proven
common structure, only when it replaces more than it adds" intent.

**Second-consumer (Phase 4 gate): DONE — `community-pulse` `1d3a197`.** It adopts
the `CommandSubmit → authority → sink → CausalReceipt` shape for the attention-
budget transition (`set_budget`): the engine is the authority (clamps the budget
from live state), the SQLite `settings` upsert is the write-only sink, and the
terminal `CausalReceipt` is the monotone ack. In-process (no socket — community-
pulse is single-process); the contract is the shape, which is what the gate
needs. 3 tests green (applied round-trip + sink fired, authority clamps,
malformed payload fails closed). The two consumers (agent-doc closeout +
community-pulse budget) now share the shape → the Phase 4 promotion review is
unblocked.
durable transitions. Either satisfies the "one unrelated application" entry
condition; only after it (plus agent-doc) both pass Phase 3 enforcement does the
promotion review begin.

## Proof matrix

| Claim | Required proof |
| --- | --- |
| Live state remains authoritative | No persistence query/CAS in the migrated hot path |
| Projection persistence is coherent | One write for the settled batch value |
| History persistence is lossless | Replay covers every epoch after the durable cursor |
| Retry is safe | Repeated epoch writes are idempotent |
| Ack cannot regress | `durable_through` is monotone |
| Restart is safe | Cold hydration reconstructs the last acknowledged state, then replay resumes |
| Ephemeral data stays ephemeral | Compile-fail durability-marker test |
| The feature is not bloat | No backend in Lazily; net code reduction after two consumers |

## Recommendation

Do phases 1 and 2 now. Treat phases 3 and 4 as evidence-gated follow-ups.

That gives agents a clear authority rule immediately and tests it against the
agent-doc failure mode that motivated the work. It avoids prematurely adding a
new Lazily subsystem when the existing effect, topic, outbox, durability, and
receipt primitives may already be sufficient.

## Progress (live log — read this first when resuming)

All work below is committed and pushed to `main`. Tag: `#lzdurablesink`.

### Architecture decision (binds the remaining work)

Closeout authority routes through **lazily `command-plane-v1`**
(`CommandSubmit` + terminal `CausalReceipt`), **not** a bespoke
`send_ndjson_request_to_actor` verb. A bespoke `closeout_phase_advance`
socket-verb was drafted and reverted (`send_ndjson_request_to_actor` is the
wrong primitive — it reinvents idempotency/generation/supersession the command
plane already provides). The durable-sink shape holds: the phase
`turn_intent_checkpoint` + phase fact(s) are the effect sink
(`append_apply_state_event` = idempotent SQLite insert + in-memory apply), and
the `CausalReceipt` is the monotone acknowledgement.

Event labels are typed, not free-text: `CloseoutPhaseEvent` is the label;
`Committed(CommitObservation)` carries the closed stable/no-op commit
vocabulary; `Abandoned` carries a named `reason` field; `last_event_label()`
derives the legacy `last_event` string.

### Phase 1 — DONE (guidance before API)

- lazily-spec `35563c1`: `docs/durable-sinks.md` + `protocol.md` § Durable effect
  sinks + `SUMMARY.md` entry.
- lazily-formal `6e46ce2`: `LazilyFormal/DurableSink.lean` (monotone
  `durable_through`, projection batch-coalesces to settled epoch, lossless
  history replay, sink-failure-never-regresses-authority, cold hydrate resumes).
  `make check` green.
- lazily-rs `2095d9c`: README durable-effect-sinks section.
- agent-doc: `AGENTS.md` durable-sink section + the architecture-review prompt.

### Phase 2 — foundations + closeout authority DONE; live transport + migration PENDING

Done in agent-doc (all on `main`):

- `1c4c2a04e` — `commit_barrier_for_closeout` reads the live in-memory projection
  (`runtime.document_state_projection`), no `state.db` replay, no self-RPC.
- `bbb69746d` — `reconstruct_cycle_state(&DocumentStateProjection)` pure helper
  (shared by SQLite `load` and the actor).
- `dda683a30` — pure fact builders `build_turn_intent_checkpoint_event` +
  `build_closeout_projection_event` (+ pub `CloseoutProjectionEvent`).
- `6e5b0b635` — client side: `build_closeout_advance_submit` +
  `closeout_advance_receipt` + `CloseoutAdvancePayload` on command-plane-v1.
- `4a5d584a4` — typed event labels (above).
- `9ec4b4b00` — `service_closeout_advance` authority (decode → reconstruct from
  live projection → `decide_*` → `append_apply_state_event` sink → `CausalReceipt`)
  + `decide_write_applied`; in-process test.
- `f2dc95061` — `decide_response_captured` / `decide_committed` / `decide_abandoned`
  (+ `CloseoutDecision` for the tri-state) ; `service_closeout_advance` routes all
  four transitions; tests cover `PreflightStarted→WriteApplied→Committed` + `Abandoned`.

The `mark_*` client funcs already delegate to the shared pure `decide_*`
(behavior-preserving; 50 cycle-state-io tests green; clippy `--all-targets` clean).

### Remaining todos (Phase 2)

1. **DONE — `91e8b5715`** Wire the live `CommandTransport` over the controller
   socket. Server: a `command_plane_submit` controller-request arm routes a lazily
   `CommandSubmit` (riding `diagnostic_payload`) by `(namespace, name)` to
   `service_closeout_advance`; unknown names → terminal `rejected` receipt; foreign
   namespaces refused; `#af88` stale-binary guard reused. Client:
   `ControllerCommandTransport: lazily::CommandTransport` over the one-line socket
   + `submit_closeout_advance_command`. `#[allow(dead_code)]` dropped; clippy
   `--all-targets` clean; nextest 219 passed (3 new dispatch tests). Pushed to `main`.
2. **DONE — `2047beab6` + `9bef9a98e` + `8e7f711ce` + `af6f61a69`** Migrate the
   `mark_*` clients to submit `closeout_advance` commands — at the chokepoint.
   The closeout command-plane vocabulary moved down to `agent-doc-cycle-state-io`
   (+lazily ipc dep); `controller-io` re-exports + keeps `decode_closeout_advance_payload`.
   **All four transitions route through the command plane** (dual-path: controller
   live → submit; no controller → local fallback), including `mark_committed`,
   whose label rides the `event_label` payload field so diagnostic commit labels
   (`capture_committed`, `repair_applied`, …) survive verbatim.

   **Key dogfooding gotcha (resolved):** the controller runs the *installed*
   `agent-doc` binary (spawned detached), and `make check` does not reinstall
   first. An earlier "regression" in `mark_committed` was a stale installed
   binary, not a logic bug — there was no write-bypass prerequisite. With a fresh
   `make install`, the session-check recovery test passes and `make check` is
   green. Rule: after any controller/command-plane code change, `make install`
   before `make check` (or before trusting an integration test that spawns the
   controller).
3. **DONE — `e0978409f`** Migrate `closeout_owner_claim`/`release` onto the
   command plane. They are coordination CAS ops whose authority result is the
   typed outcome (`CloseoutOwnerClaimOutcome`: Acquired/HeldByOther/CycleSuperseded;
   release bool), not a coarse Applied/Rejected receipt — so the dispatch returns
   the command's typed result (advance→receipt unchanged, claim→outcome,
   release→bool). Shared `run_closeout_owner_*` cores extracted; bespoke verbs +
   command-plane services both call them. Vocabulary in cycle-state-io. clippy
   clean; nextest 272 passed.
4. **DONE — `3d29324d0` + `8e7f711ce`** Read-only inspection from the live
   projection. `load_document_projection` (covers `load` /
   `load_with_closeout_projection` / `load_closeout_projection` /
   `load_document_disk_write`) prefers the controller's live in-memory projection
   when a controller is live, replaying cold state.db only for the actorless
   boundary. New read-only `document_state_projection` verb. Two robustness fixes
   the live flows surfaced (both genuine prerequisites for a live-projection
   authority): an **in-controller self-RPC guard** (`set_in_controller_request`
   thread-local, set by the serve entry — the controller's own request handling
   must not round-trip to the socket it is serving), and a **stale-socket →
   state.db fallback** (fall back on any Connect error, not only an absent socket
   file). make check green.

### New foundation primitives (from todo 4, available to the rest of Phase 2)
- `agent_doc_cycle_state_io::set_in_controller_request(bool)` — thread-local
  guard against self-RPC for any client helper invoked during controller request
  handling. Reuse it when migrating more hot-path calls (todo 5).
5. **DEFERRED by design — the flock is the correct primitive here.** The per-
   consume `flock` (`agent-doc-queue-io/src/queue_consume.rs:49` `acquire_doc_lock`,
   6 sites + `agent-doc-write-runtime-io/src/lib.rs:2478`) guards a **document-file
   read-parse-write TOCTOU** ("prevent concurrent edits from invalidating parsed
   offsets"). It is NOT a closeout-authority arbitration, so the `#lazily-hot-path`
   rule (which scopes to "flock … never document closeout") does not forbid it.

   A controller-held replacement lock was designed and rejected on engineering
   merits:
   - **Crash recovery regression** — `flock` auto-releases on process death (kernel
     fd cleanup); a lease-based controller lock holds until TTL, blocking other
     consumers after a crash.
   - **Controller serialization** — `queue_consume.rs:365-370` explicitly warns
     "Never hold the document lock across that bounded RPC: a slow/unavailable
     controller would otherwise serialize every preflight/commit contender behind
     the timeout." Routing the lock through the controller is exactly that.
   - **Reimplements a correct kernel primitive** with lease/TTL/RPC complexity for
     no benefit.

   **Decision: keep the flock for document-file edit TOCTOU.** Revisit only if the
   controller becomes the single document-file writer (projecting document-queue
   state), at which point no lock is needed.
   **Cleanup DONE — `c6549dff2`:** consolidated the duplicate `acquire_doc_lock`
   definitions (queue-io + write-runtime-io) into one shared
   `agent_doc_fs::acquire_doc_lock`, with the design decision documented in its
   doc comment. Behavior-preserving; `make check` green.
 6. **DONE — `8e7f711ce`** Full `make check` green (clippy `--all-targets` +
    nextest + `sim-medium` + `version-sync` + `lean` + `tla`). (Pre-existing flake
    note: the `crdt_checkpoint_*` pair can fail under `cargo test --lib` from
    cross-test leakage, `#wsflake2`, but passes under nextest isolation.)

   **Dogfooding rule (from the `mark_committed` resolution):** `make install`
   before `make check` whenever controller/command-plane code changed — the
   integration tests spawn the *installed* binary as the controller.


### Resume hint

Todos 1–4 + 6 are done; `make check` green on agent-doc `main`. The command-plane
authority is live for closeout: `mark_write_applied`/`mark_response_captured`/
`mark_abandoned` + owner claim/release route through it; `load*` reads the live
projection; the self-RPC guard + stale-socket fallback are in place. **Phase 4
second consumer DONE** — `community-pulse` `1d3a197` adopts the shape for
`set_budget`; the two consumers now share it, unblocking the Phase 4 promotion
review. Phase 3 enforcement is largely present in lazily (compile-fail +
property laws); its agent-doc architecture TEST is blocked on Phase 2 cleanliness.

**Follow-up A DONE — `af6f61a69`:** `mark_committed` is on the command plane
(all four transitions now route through it). The earlier blocker was a stale
*installed* binary (the controller runs the installed `agent-doc`; `make check`
doesn't reinstall) — not a write-bypass prerequisite. Rule: `make install` before
`make check` after controller/command-plane changes.

**One follow-up remains — todo (5):** route queue consume through the authority,
then drop the per-consume `flock` (6 sites). **Design driven + DECISION: keep the
flock** — it is the correct primitive for document-file edit TOCTOU (kernel-
enforced, auto-release on crash). A controller-held replacement was designed and
rejected: it regresses crash recovery (lease holds after death; flock releases)
and serializes consumes behind the controller (`queue_consume.rs:365-370` warns
against exactly that). `#lazily-hot-path` scopes its flock ban to closeout, not
document-file edits. **Cleanup landed (`c6549dff2`):** consolidated the duplicate
`acquire_doc_lock` into one shared `agent_doc_fs` primitive. Phase 2 is as uniform
as it gets without making the controller the single document-file writer.

**Phase 4 promotion review: DEFER** (recorded above) — two consumers share the
shape but criterion 5 (net code reduction) isn't met yet; re-review after a third
consumer.
