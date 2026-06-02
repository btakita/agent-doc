# Plan: explicit-position `--pending-add` primitives

## Motivation

Ordering a multi-item `--pending-add` batch in `agent:backlog` is a recurring footgun.
Two separate agents (open-code.md `#adcr` waves, commit `f94e7851c`; corky.md crate-split
phases, commit `fc8d9b3bc`) both passed items in reverse expecting prepend-flip, and both
produced reverse-of-intended order. The root issue is that **the only insertion position is an
implicit default (front)**, so building an ordered sub-sequence requires either reverse-reasoning
about batch semantics or a follow-up `--pending-reorder`.

The semantics themselves are contested:

- `SKILL.md` "Pending capture rule": *Put new items at the beginning of `agent:backlog`.*
- Binary behavior (test `pending_add_multiple_items_keep_sequence_order_at_front`): a multi-item
  batch keeps **argument order** at the front (`notify.rs` iterates `pending_add.iter().rev()`
  and inserts each at position 0).
- Existing bug `#pending-add-append-not-prepend` (plan `plan-pending-add-prepend-semantics.md`)
  recommends Option A — strict prepend — which would make multi-add a stack (last arg on top).

So three conventions are in tension (front-arg-order vs strict-prepend vs SKILL.md "beginning"),
none of them removes the need for the agent to reason about argv direction. Picking any single
implicit convention still leaves ordered-batch insertion fragile.

## Design principle

The backlog is **not a stack or a queue** — it is a priority-ordered pool with **ID-based
random-access consumption** (`--done`, `--pending-gate` reference `#id`, never position). FIFO
execution discipline already lives in the separate `agent:queue` (head-consumed by the auto-loop).
Therefore **position in the backlog encodes author intent and should be set explicitly when it
matters**, not inferred from argv direction.

## Recommendation

Keep `--pending-add` as the cheap front-insert default (single-item capture stays ergonomic), and
add explicit-position primitives so ordered insertion is unambiguous:

- `--pending-add-after <id> "<text>"` — insert immediately after an existing item (the "adjacent
  to predecessor" rule the SKILL already describes, made first-class). Repeatable; chaining
  `--pending-add-after A "B" --pending-add-after B "C"` builds A→B→C deterministically.
- `--pending-add-before <id> "<text>"` — symmetric counterpart.
- `--pending-add-back "<text>"` (alias `--pending-append`) — insert at the end of the active
  backlog, for low-priority captures that should not jump the queue.

`--pending-add` itself should also be made **deterministic and documented** for the multi-item
case — adopt `#pending-add-append-not-prepend` Option A (strict prepend, first arg ends deepest)
OR keep front-arg-order, but pick one, assert it in a test, and state it in `SKILL.md` + `--help`.
The explicit-position flags make the choice low-stakes because agents that care about order use the
explicit primitive instead of relying on the default.

## Files

- `src/agent-doc/src/pending.rs` — already exposes `op_insert_item_first`; add `op_insert_after`,
  `op_insert_before`, `op_insert_last` (ID lookup + positional splice; error on unknown id).
- `src/agent-doc/src/notify.rs` — the `pending_add.iter().rev()` loop; wire the new op vectors and
  make the existing default's order explicit + commented.
- `src/agent-doc/src/write.rs` / `main.rs` (clap derive) — new repeatable flags
  `--pending-add-after`, `--pending-add-before`, `--pending-add-back`; value parsing for the
  `<id> <text>` pairs (mirror `--pending-add-to`'s two-value form).
- `SKILL.md`, `runbooks/pending-ops.md`, `README.md`, `SPEC.md` — document the priority-pool model
  (not stack/queue), the explicit-position flags, and the finalized default ordering. Resolve the
  SKILL.md "beginning" wording against the chosen default.
- Tests in `notify.rs` — one per new flag (after/before/back), plus an updated/renamed default-order
  test that the docs cite verbatim.

## Acceptance

- `--pending-add-after <id>` chains build a deterministic ordered sub-sequence with no reorder pass.
- `--pending-add-back` lands an item at the tail without disturbing the head.
- `SKILL.md`/`--help` state the `--pending-add` multi-item default and it matches the test.
- `make check` green; `audit-docs` clean on the touched instruction surfaces.

## Relationship to existing items

Supersedes the "future flag" note in `plan-pending-add-prepend-semantics.md`. Implement alongside
`#pending-add-append-not-prepend`: that item finalizes the default-ordering decision; this item adds
the explicit-position escape hatches so ordered batches never depend on the default at all.
