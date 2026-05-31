# Plan — backlog→queue sync attribute (`#backlog-queue-sync-attribute`)

## Goal

Add a `queue=sync|append|prepend` attribute to `agent:backlog` / `agent:icebox`
so the binary regenerates `agent:queue` from the backlog item list, automating the
manual "regenerate the queue to the new priority" step. Composes with
`#backlog-priority-attribute` (prioritize the backlog first, then regenerate the
queue in that order).

- `queue=sync` — replace `agent:queue` content with a `do [#id]` list for every
  active backlog item.
- `queue=append` — add backlog item ids not already in `agent:queue`.
- `queue=prepend` — prepend those ids.

## Why this needs a design decision first (gated)

1. **Direct conflict with `#queue-user-edit-overwrite` (open review item).** That
   item exists because user-authored `agent:queue` edits were being silently
   overwritten by queue convergence. `queue=sync` **replaces** the queue — exactly
   the clobber that safety guards against. The two cannot both be naive. The sync
   must be **deliberate and provenance-aware**, never a silent every-write rewrite
   that eats a user's hand-edited queue head.
2. **Trigger semantics.** If the attribute regenerates on *every* write, it fights
   manual queue edits and the auto-loop's head consumption (the queue is being
   drained head-first while the backlog still lists drained ids). Options:
   - One-shot: the attribute is **consumed/cleared** after a single regeneration
     (declarative "sync once now"), so it never silently re-fires.
   - Command-driven: a `agent-doc queue sync|append|prepend` (or
     `--queue-sync`) flag the operator runs explicitly; the attribute is just the
     default mode for that command.
3. **Which ids?** Only active (`[ ]`) backlog items, or also gated (`[/]`)? Done
   items must be excluded. Already-consumed/struck queue heads must not be
   re-added (interaction with `#drained-done-queue-clear`).

## Recommended design (for confirmation)

- **One-shot, provenance-aware.** `queue=sync` (etc.) is applied once by a
  dedicated maintenance step, then the attribute is **rewritten to a neutral
  marker** (or removed) so it does not silently re-fire and cannot clobber a later
  user queue edit. `append`/`prepend` only add ids **absent** from the current
  queue (set-diff), so they never remove a user-added head.
- **`sync` reconciles, not blindly replaces.** Even `queue=sync` should preserve a
  user-added queue head that has no backlog id (free-text head) rather than delete
  it — reuse the `#queue-user-edit-overwrite` provenance/epoch tracking when it
  lands, or gate `sync` behind that fix.
- **Active-id source.** Default to active `[ ]` backlog items in document order
  (which, with `#backlog-priority-attribute`, is already priority order). Exclude
  `[x]`/done; skip ids already present (idempotent).
- **Ordering pairs with `#backlog-priority-attribute`.** Prioritize first
  (`sort=priority`), then `queue=sync` emits `do [#id]` in that order.

## Tests (when implemented)

- `sync` emits `do [#id]` for each active backlog id in order; gated/done excluded.
- `append`/`prepend` only add absent ids; a user-added free-text head survives.
- One-shot: the attribute does not silently re-fire on the next write.
- Composition: `sort=priority` + `queue=sync` emits the prioritized order.
- Regression vs `#queue-user-edit-overwrite`: a hand-edited queue head is never
  silently dropped by a sync.

## Non-goals / dependencies

- **Do not** ship a naive every-write replace — it reintroduces
  `#queue-user-edit-overwrite`. Gate `queue=sync` on (or build it atop) that
  fix's provenance tracking.
- Confirm trigger model (one-shot attribute vs explicit command) and the
  active/gated id source before implementing — the gating decision.
- Co-designed with `#backlog-priority-attribute`
  (`tasks/agent-doc/plan-backlog-priority-attribute.md`).
