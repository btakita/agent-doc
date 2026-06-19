# Plan — #ftstrike: strike addressed free-text queue items regardless of position

## Problem

Operator report (agent-doc-bugs2.md queue, 2026-06-19): "My free-text queue items are
not immediately struck as if they are addressed."

Free-text (non-`[#id]`) queue items are struck only when they are the **leading** head and
get consumed on `finalize` (the normal leading-head consume). A free-text item sitting
**behind** another head (e.g. behind `do [#fullboundary]`) is never struck even after the
response addresses it, so the operator cannot tell which of their typed bug-reports have
been answered. This is the promoted form of `#733r` Mechanism A (free-text head strike
regardless of position), previously deferred as a "future nicety."

Contrast: `[#id]` heads already strike regardless of position via
`strike_done_queue_head_prompts` (matches `agent:done` ids + review-gated ids).

## Goal

When a response addresses a free-text queue item, strike that item (`~~…~~`) on the same
finalize, regardless of its position in the queue — mirroring the id-head behavior — without
re-introducing structural corruption or striking items that were NOT addressed.

## Approach (design to validate)

1. **Addressed-detection for free-text heads.** A free-text head is "addressed" this cycle
   when the committed response contains a `### Re:` section whose quoted prompt block matches
   the head's text (normalized: strip leading `:pushpin:`/`-`, fenced log blocks, whitespace).
   Reuse the existing prompt→`Re:` association the reconcile path already computes for the
   exchange tail rather than a fresh fuzzy match.
2. **Strike pass.** Extend the queue-maintenance strike pass (alongside
   `strike_done_queue_head_prompts`) to strike matched free-text heads at any index, not just
   the leading one. Leading-head consume stays as-is for the drained head.
3. **Safety.** Only strike on a *positive* match to an addressed `Re:` section — never strike
   an unaddressed item. Preserve the `#semmerge` per-structure constraint: mutate only the
   `queue` component nodes, never reflow surrounding structure.
4. **Optional refinement (from #733r):** an explicit `[filed:#id]` head→backlog linkage marker
   for the case where a free-text report is filed as a backlog item rather than answered inline;
   strike the head when its linked id is added/done.

## Tests

- Unit (preflight/maintenance): free-text head behind an id head is struck once its `Re:` match
  lands; an unaddressed free-text head is left intact; duplicate free-text heads still collapse.
- SimWorld: operator adds two free-text reports behind a leading head, response addresses both →
  both struck, leading head consumed independently.

## Out of scope

- No change to id-head striking (already correct).
- No change to the leading-head consume contract.
