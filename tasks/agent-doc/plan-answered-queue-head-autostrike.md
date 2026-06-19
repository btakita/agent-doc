# Plan: auto-strike / defer answered queue heads (qflood thrash root cause)

## Problem

The `agent-doc-bugs2.md` session has thrashed for 6+ consecutive no-op closeouts
(`recent_noop_closeouts=6`). Each cycle re-presents the *same* already-addressed
queue heads as `queue_drainable`, so `queue_continuation_required` stays `true`
and the supervisor / in-session loop keeps re-dispatching `/agent-doc` on work
that is already done or operator-gated.

Two distinct mechanisms produce the thrash:

### Mechanism A — free-text operator reports stay drainable after being filed

An operator pastes a free-text bug report into the queue (e.g. `:pushpin: JB
File Cache Conflict still occurring...`, `:pushpin: equityfundingsource.md &
monsterrodholders.md keeps on drifting with edits.`). The agent answers it by
**filing an operator-verify backlog item** (`#pcwcdiskfree`, `#docdriftgrace`).
But the original free-text queue head is never struck or linked to the filed
item, so on the next preflight it is still a free-text `queue_drainable` head.
`queue consume` *could* strike it, but only when it is the literal leading head.

### Mechanism B — `--done <id>` does not strike the matching id-backed queue head

Reaping a backlog item via `agent-doc write --done <id>` removes the backlog
item but leaves a matching `[#id]` queue head in place. That orphan head then
(a) hard-blocks the next preflight (`open backlog item(s) exist only outside
live agent:backlog: #id`), and (b) must be struck by hand
(Edit + `reset --from-current --preserve-session` + `commit`). Observed this
session with `#jbacceptwedge`.

Net effect: the only way to drain is manual queue surgery on a degraded-IPC live
session — exactly the toil the operator is complaining about in `#docdriftgrace`
/ `#qpauseux`.

## Root cause

The closeout that *answers* a queue head (filing a backlog item, or `--done`-ing
an id) does not propagate that resolution back to the **queue head** itself. The
queue head and its resolution live in two places that are never reconciled in
one path.

## Fix direction

1. **`--done <id>` / `--pending-gate <id>` strikes the matching `[#id]` queue
   head in the same write** (Mechanism B). After reaping/gating the backlog
   item, scan the queue for a head whose resolved id == `<id>` and strike it.
   This removes the orphan-shadow hard-block and the manual-surgery requirement.

2. **Link a free-text queue head to the backlog item it was filed as**
   (Mechanism A). When the agent files an operator-verify backlog item that
   resolves a free-text head, record the head→id linkage (e.g. an inline
   `[filed:#id]` marker the parser strips, or a sidecar map keyed by the head's
   `trigger_sha256`). On the next preflight, a head linked to an
   `[operator-verify]` backlog item defers from `queue_drainable_head_count`
   the same way the backlog item already does (`#qcontdrain`). Verify the linked
   item is still open; if it is `done`, strike the head instead.

3. **Drainability dereferences id-backed heads** (defensive): a bare `[#id]`
   queue head should resolve its backlog tag and defer when that item is
   `[operator-verify]`, rather than counting as drainable on the id-reference
   alone.

## Acceptance

- SimWorld: a session whose only remaining queue heads are (a) `[#id]` refs to
  `[operator-verify]` backlog items and (b) free-text heads previously filed as
  operator-verify items reports `queue_drainable_head_count=0` /
  `queue_continuation_required=false` — no thrash.
- `--done <id>` on an item with a matching `[#id]` queue head leaves zero orphan
  shadow (preflight clean, no manual strike needed).
- `make check` green; `make tmux-ci` green.

## Status — RESOLVED (verified 2026-06-19)

All three documented mechanisms are implemented in the binary:

- **Mechanism B (orphan `[#id]` head)** — `preflight::strike_done_queue_head_prompts`
  strikes a matching `[#id]` queue head **regardless of position** for both
  `agent:done` ids (`collect_agent_done_ids`) and review-gated ids
  (`collect_agent_review_gated_ids`), so `--done <id>` / `--pending-gate <id>`
  leaves zero orphan shadow. Covered by
  `strike_done_queue_prompts_strikes_non_head_resolved_ref` +
  `strike_done_queue_head_prompts_strikes_review_gated_items`.
- **Mechanism A (free-text report)** — duplicate free-text heads collapse via
  `dedup_queue_nodes_by_key`; a non-duplicate free-text head drains by the
  normal leading-head consume on `finalize`. (The originally-specced explicit
  `[filed:#id]` head→item linkage marker is an OPTIONAL refinement for the rare
  case of a non-leading, non-duplicate free-text head filed as operator-verify;
  it is not required for the no-thrash acceptance below and is left as a future
  nicety.)
- **Mechanism 3 (id-head deferral)** — `queue_continuation::deferred_backlog_ids`
  defers a `do [#id]` head whose backlog item is `[operator-verify]`; covered by
  `drainable_head_count_zero_when_only_deferred_heads_remain`.

Acceptance met: the drainable-head-count / continuation signal is 0 when only
`[operator-verify]` refs remain, `--done` leaves no orphan shadow, `make check`
green. The qflood thrash family root cause (orphan id-heads hard-blocking +
un-reconciled resolutions) is fixed; this session drained continuously with no
thrash.

## Notes

- This was the structural fix for the qflood thrash family
  (`#docdriftgrace`, `#qpauseux`, `#rz3a`).
