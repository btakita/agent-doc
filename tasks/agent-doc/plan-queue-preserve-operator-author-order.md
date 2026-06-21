# Plan: preserve operator-authored queue order (no auto-bubble, no convergence duplication)

**Backlog id:** `#qauthorder`
**Class:** `#agent-doc-bug`, queue-convergence-core (focused cycle — intersects several existing queue invariants).
**Operator report (this session, 2026-06-21, `agent-doc-bugs2.md`):**

> Queue items that are added by operator should be handled in the queue order defined in the document...not automatically bubbled to the top.

## Observed churn (live proof in this doc)

A single preflight on `agent-doc-bugs2.md` produced both symptoms:

1. **Convergence duplicated a free-text operator line.** The operator added one
   `Queue items that are added by operator should be handled...` line; the semantic
   diff shows it inserted **twice** (`node_key queue:3:ft-c3831b31:0` **and**
   `queue:3:ft-c3831b31:1`, same `item_id=ft-c3831b31`). The existing
   `#qdedupsync` collapse (`dedup_pin_variant_do_heads`) only de-dups `do [#id]`
   heads; **free-text operator lines have no dedup**, so a CRDT/backlog-sync
   convergence that re-emits the line leaves a visible duplicate.

2. **Auto-pin injects a `:pushpin:` the operator never typed.** `annotate_manual_queue_additions`
   (`#7r2s`, `preflight/maintenance.rs` ~L1476, `queue.rs:825`) calls
   `apply_operator_pin` on every operator-added unpinned line so the downstream
   `sort_prompts_by_priority` position-lock holds its slot. Side effect: the
   operator sees a pin glyph they didn't author, and the line is now treated as a
   tier-0 operator pin for all later passes.

## Root-cause map (existing machinery to reconcile with — do NOT regress)

- `sort_prompts_by_priority` (`queue.rs:914`) — operator pins (tier 0) are
  **position-locked** (`#queue-operator-pin-position-lock`); only agent pins
  (tier 1) and the unpinned tail (tier 2) reorder. Backlog-sourced unpinned
  prompts are held to a later group (`#backlog-queue-append-stable`).
- `annotate_operator_priority_reorders` (`queue.rs:764`) — re-pins a prompt the
  operator manually moved vs the snapshot ("pinned manually reordered prompt(s)").
- `annotate_manual_queue_additions` (`queue.rs:825`, `#7r2s`) — auto-pins a
  brand-new operator line so the priority sort doesn't sink it under `queue`-attr
  backlog items.
- `dedup_pin_variant_do_heads` (`queue.rs`, `#qdedupsync`) — collapses
  pin-variant `do [#id]` duplicates; **free-text-blind**.

The design *intent* already matches the operator (authored slot is preserved via
position-lock). The bug is the **mechanism**: holding the slot requires injecting a
visible pin, and the free-text path has a dedup hole that surfaces duplicates.

## Fix direction

1. **Preserve operator-authored document order WITHOUT injecting a pin glyph.**
   Replace the "auto-pin to hold the slot" hack with an order-preserving anchor
   that does not mutate the line's visible text. Options:
   - Track operator-authored positions as a separate position-lock set keyed by
     the line's stable identity (`strip_priority_markers`), so
     `sort_prompts_by_priority` anchors them at their document slot the same way
     tier-0 does, but without rewriting the text to add `:pushpin:`.
   - Equivalent: an internal "authored-order" tier that is position-locked but
     renders no marker.
2. **Dedup free-text operator lines on convergence.** Extend the dedup pass (or
   `sync_backlog_into_queue` / CRDT re-emit path) to collapse byte-identical
   free-text queue lines (after `strip_priority_markers`) to a single instance at
   the earliest authored slot — the free-text analogue of `dedup_pin_variant_do_heads`.
   Carve-out: preserve a genuinely intended exact duplicate only if there is a
   signal it was authored twice (default: collapse — free-text repetition is
   overwhelmingly a convergence artifact, as proven here).
3. **Never bubble.** Assert that no convergence/sync/sort pass moves an
   operator-authored line above a line the operator placed before it.

## Acceptance (SimWorld + unit)

- An operator adds an unpinned free-text line at slot N → after preflight it stays
  at slot N, **with no `:pushpin:` added**, and appears exactly once.
- An operator adds `do [#id]` mid-queue (unpinned) → stays at its slot, not
  bubbled above earlier operator lines, no duplicate.
- A backlog→queue sync that re-emits an already-present free-text line produces no
  duplicate.
- Existing position-lock / append-stable / DAG tests stay green (no regression to
  `#queue-operator-pin-position-lock`, `#backlog-queue-append-stable`,
  `#queue-auto-dag-priority`).
- `render(parse(x))` fixed-point family extended with a free-text-duplicate input.

## Why focused-cycle, not inline

This is the exact convergence machinery actively churning this live doc; it
intersects `#7r2s`, `#queue-operator-pin-position-lock`, `#backlog-queue-append-stable`,
`#qdedupsync`, and the DAG sort. Editing it inline in a drift-heavy live session
risks worsening the churn. Pure preflight/queue logic → unit + SimWorld provable,
no live editor needed. Goes live at the next `make install` (held by the
STOP-cutting-releases freeze).
