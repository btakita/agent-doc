# Plan — backlog priority attribute (`#backlog-priority-attribute`)

## Goal

Let the binary maintain `agent:backlog` / `agent:icebox` / `agent:queue` in
priority order automatically, instead of the manual `--pending-reorder` pass done
by hand in `#next-steps` cycles. Composes with `#backlog-queue-sync-attribute`
(regenerate the queue from the prioritized backlog) and is a sibling of `#ah0s`
(explicit-position `--pending-add` primitives, already shipped).

## Why this needs a design decision first (gated)

1. **Tension with `#ah0s`.** `#ah0s` just established that the backlog is a
   priority-ordered *pool* where **position encodes author intent**, set
   explicitly (`--pending-add-after/-before/-back`). Auto-sorting by a priority
   attribute partially overrides author-set position. The two must compose, not
   fight: auto-sort should be **opt-in per component** so manual order stays the
   default.
2. **Marker syntax is a UX decision.** `PendingItem` has no priority field today;
   `[recommended]` is free text in the item body. Options, none obviously best:
   - Per-item leading token `[p1]`..`[p9]` (low number = higher priority).
   - `priority=high|med|low` token (mirrors the `id=<custom>` prefix grammar).
   - Reuse/extend `[recommended]` into a tiered scheme.
   Each changes how items render and how `[recommended]` coexists.
3. **Sort trigger + stability.** When does the binary sort — every write? a
   `--pending-reorder --by-priority` command? on preflight maintenance? And the
   sort must be **stable** (items of equal priority keep author order) and must
   not disturb gated/done partitioning.

## Recommended design (for confirmation)

- **Opt-in component attribute** `<!-- agent:backlog sort=priority -->` (and same
  for `icebox`/`queue`). Absent ⇒ manual order (today's behavior, honoring
  `#ah0s`). Present ⇒ the binary keeps the component stably sorted by priority.
- **Per-item priority token** in the canonical prefix grammar, alongside
  `id=<custom>`: `priority=<1..9>` (1 = highest). Items without a priority sort
  **after** prioritized ones, preserving author order among themselves (stable).
  `[recommended]` stays orthogonal free-text.
- **Sort applied by `run_pending_maintenance`** (preflight + finalize already call
  it) when the component carries `sort=priority`, so prioritization is maintained
  every cycle without a manual reorder. A new `pending::op_sort_by_priority(body)`
  pure function does the stable sort; `--pending-add`/`--pending-add-after` etc.
  insert as today and the next maintenance pass re-sorts.

## Tests (when implemented)

- `op_sort_by_priority` stable-orders by `priority=N`, ties keep author order,
  unprioritized items sort last, gated/done partitioning preserved.
- Component without `sort=priority` is never reordered (composition with `#ah0s`).
- Maintenance re-sorts on write when the attribute is present; idempotent.
- Parse round-trip: `priority=N` survives render and is not duplicated.

## Non-goals / open questions

- Do not auto-sort components lacking the explicit `sort=priority` attribute.
- Confirm the marker syntax (`priority=N` vs `[pN]`) and the highest-priority
  direction before implementing — this is the gating decision.
- Pairs with `#backlog-queue-sync-attribute`: once the backlog is prioritized,
  `queue=sync` regenerates the `agent:queue` `do [#id]` list in that order.
