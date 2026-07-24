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
