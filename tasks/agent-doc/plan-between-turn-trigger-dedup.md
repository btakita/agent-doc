# Plan — between-queue-turn trigger enqueue must de-duplicate (one /clear, one /agent-doc <FILE>)

Bug id: `#qdedup`
Tag: `[clean-session]` (PCP/Supervisor dispatch path; must not run under the live route-owned supervisor)

## Operator-stated symptom (2026-06-13)

Prompts are still being entered into the agent *during* an active turn. The
operator wants: **stop the running agent-doc queue drain once a queue turn is
complete, then insert the commands into the inactive agent between queue turns.**
The turn ended with a garbled, accumulated trigger:

```
/clear /agent-doc tasks/agent-doc/agent-doc-bugs2.md/agent-doc tasks/agent-doc/agent-doc-bugs2.md
```

i.e. multiple `/clear` and multiple `/agent-doc <FILE>` triggers concatenated
into one line because successive between-turn enqueues appended without
de-duplication. Operator ask: "enqueue unique (de-duplicated … only one
`/clear` allowed, only one `/agent-doc <FILE>` allowed) instructions. These must
land once as part of the PCP/Supervisor between-queue-turn stage."

## Relationship to existing items

- `#qflood` / `#qflood2` (review, `plan-queue-dispatch-flood.md`) — dual-drain
  coalesce exit-1. Related but distinct: that is *collision reporting*; this is
  *accumulated duplicate trigger text* in a single between-turn enqueue.
- `#qchurn` (`plan-go-queue-stall.md`), `#goqueuestall` (done) — no-op churn /
  undrainable heads. Related (same idle-watch dispatch path) but distinct.
- `#kp5z` / `drain-claim` lease — `/loop` owns the drain so the supervisor
  defers. This bug is the supervisor's *own* between-turn enqueue not
  de-duplicating when it does inject.

## Code anchors (re-baseline first — submodule tree is dirty)

- Supervisor idle-queue watch / between-turn dispatch:
  `agent-doc-orchestration/src/start/idle_watch.rs`,
  `agent-doc-orchestration/src/start/decisions.rs`
  (`idle_queue_drain_decision`). These carry uncommitted work right now.
- Trigger composition / send: route dispatch (`route.rs send_command_unchecked`),
  `session_actor_cmd.rs` clear/clear-cooldown delivery.
- Clear-cooldown / resume: `plan-clear-cooldown-go-drain-resume.md`
  (`#clearcontresume`), `plan-post-clear-auto-queue-suppress.md`.

## Fix direction

1. The between-turn enqueue must compose the next trigger as a **set**, not an
   append: dedup so at most one `/clear` and at most one
   `/agent-doc <FILE>` (per FILE) land, in the correct order
   (`/clear` then `/agent-doc <FILE>`), once.
2. Gate the enqueue strictly to the **inter-turn** boundary: never inject while
   a turn is active (respect the drain-claim lease + busy projection); buffer
   until the turn completes, then deliver the single deduped trigger.
3. Proof line: `between_turn_enqueue deduped=<n> kept=/clear,/agent-doc result=delivered`.

## Acceptance

- After N idle boundaries with the same FILE, the delivered trigger is exactly
  `/clear` + one `/agent-doc <FILE>`, never concatenated duplicates.
- No trigger lands while a turn is active (verified against the busy/lease gate).
- SimWorld coverage for repeated between-turn enqueues converging to one deduped
  trigger; regression test for the concatenated-trigger class.

DO NOT IMPLEMENT under the live supervisor — clean session required.
