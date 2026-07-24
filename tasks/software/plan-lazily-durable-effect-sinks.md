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

1. **Wire the live `CommandTransport` over the controller socket** (cross-cutting;
   also unlocks `supervisor_recycle` + editor routes, which are foundation-only
   today). lazily API: `CommandTransport` trait `lazily-rs/src/command.rs:523`,
   `CommandMessage` `:217`, `CommandRpcClient` `:548`. Controller serve loop:
   `serve_client` `rpc.rs:8304`, `handle_request_for_client` `:8344`. Client transport
   today: `agent_doc_state_wire::send_ndjson_request_to_actor`. Frame
   `CommandSubmit`/receipts over that socket, dispatch by `namespace`/`name` to
   `service_closeout_advance`, emit the `CausalReceipt`.
2. **Migrate the `mark_*` clients to submit `closeout_advance` commands** (they
   currently use the local `load→decide→save→append` path; the `decide_*` + builders
   are already shared). Callers of `load_with_closeout_projection` span ~45 files
   (run: `rg -n 'load_with_closeout_projection'` in agent-doc).
3. **Migrate `closeout_owner_claim`/`release` onto the command plane** — add
   `build_closeout_owner_claim_submit` + `service_closeout_owner_claim` paralleling
   `closeout_advance`; the existing bespoke handlers are
   `handle_closeout_owner_claim` `rpc.rs:9781` / `handle_closeout_owner_release`
   (decide via `decide_owner_claim` on the projection's `closeout`).
4. **Read-only inspection sites from the live projection.** Chokepoint:
   `load_document_projection` (`cycle-state-io/src/lib.rs` ≈ L700) — migrating it
   covers `load` / `load_with_closeout_projection` / `load_closeout_projection` /
   `load_document_disk_write`. Controller-internal callers must NOT self-RPC
   (`commit_barrier_for_closeout` already converted; `persist_session_actor_closeout`
   is client-called so safe). Leave `load_recent_captured_response_checkpoints`
   (legitimate bounded history read).
5. **Remove the per-consume `flock`** — `agent-doc-queue-io/src/queue_consume.rs:49`
   `acquire_doc_lock` (6 sites: 212/376/576/648/734/815) +
   `agent-doc-write-runtime-io/src/lib.rs:2478`. Replace with actor-owned dedup.
6. **Full `make check` green** — clippy `--all-targets` + nextest + `sim-medium` +
   `version-sync` + `lean` + `tla` (the `Makefile` `check` target). Known flake: the
   `crdt_checkpoint_*` pair fails under `cargo test --lib` (cross-test leakage,
   `#wsflake2`) but passes under nextest isolation — pre-existing, not caused by this
   work.

### Resume hint

The command-plane closeout foundation is complete and tested in-process. The next
concrete step is todo (1): implement a `CommandTransport` over the controller
socket so `service_closeout_advance` is reachable from a client, then (2) point
`mark_*` at it. Everything committed; `cargo check` is green on clean `main`.
