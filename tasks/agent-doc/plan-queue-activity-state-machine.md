# Plan: Queue-activity state machine (`#queuestatemachine`)

## Operator ask

> I'm still seeing duplicate queue items added by agent-doc. Do we need to have
> state machine(s) represent queue activity to ensure these sort of issues don't
> happen?

**Yes.** The recurring duplicate-injection (multiline phantom-pin flood
`#rt83qflood`, bare-id mirror dups `#qdedupsync`, pin-variant dups
`#qdup-bare-id`/`#pushpinaccum`, operator-line re-emit `#qauthorder`) are all
symptoms of the same structural gap: queue convergence is a **pile of ad-hoc
dedup passes** with no single authoritative model of what is in the queue and
why each item is there.

## Root structure today

Convergence is a sequence of independent best-effort normalizers, each patching
one duplication shape after the fact:

- `sync_backlog_into_queue` (backlog→queue mirror)
- `annotate_manual_queue_additions` / `apply_operator_pin` (operator pins)
- `dedup_free_text_heads`, `dedup_pin_variant_do_heads`,
  `dedup_bare_id_reference_heads`, `dedup_live_prompts`

Each new duplication shape needs a new dedup pass (this is why
`#rt83qflood` had to add multiline-pin keying). There is no invariant that says
"an item with identity X exists in exactly one lawful state."

## Proposal — an explicit per-item lifecycle state machine

Model each queue item by **durable head identity** (`extract_head_id` for
id-backed, normalized text key for free-text) with an explicit lifecycle:

```
Authored ─▶ Mirrored ─▶ InProgress ─▶ Answered ─▶ Struck ─▶ Reaped
   │            │                          │
   └─ operator  └─ backlog→queue           └─ exchange has Queue-prompt echo
      authored     mirror                     / `### Re:` for this identity
```

- Convergence becomes "drive each identity to its lawful state," **not**
  "append then dedup." Re-injecting an identity that already exists is a no-op
  transition, so a stale-CRDT / supervisor re-emit **cannot** create a visible
  duplicate — duplication is structurally impossible, not patched.
- Operator-authored position-lock (`#queue-operator-pin-position-lock`) becomes
  a property of the `Authored` state, preserved across transitions.
- The existing dedup passes collapse into the state machine's transition guards;
  keep them as a migration shim, then delete once the SM owns convergence.

## Phases

1. **`#queuestatemachine1`** — define the item-identity + lifecycle-state model
   (lazily-rs `Cells` is a natural substrate; ties into `#lazilystatesync` and
   the `#waitmachine` state-machine backbone) and a pure transition function with
   exhaustive `(state × event)` tests.
2. **`#queuestatemachine2`** — route convergence through the SM; reduce the ad-hoc
   dedup passes to transition guards; prove idempotence (re-emit = no-op) with a
   property test over the duplication shapes that `#rt83qflood`/`#qdedupsync`/
   `#qauthorder` each patched.
3. **`#queuestatemachine3`** — SimWorld: a stale-CRDT/supervisor re-emit storm
   converges to one item per identity; operator position-lock survives.

## Relationship to other work

Same theme as `#waitmachine` (bounded-wait SM) and `#mergestatemachine`
(editor-merge SM). These three are the operator's "represent X as a state
machine so races/dups/hangs become impossible" direction — a shared state-machine
backbone, ideally on the lazily-rs reactive substrate.
